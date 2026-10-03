//! Port of the path helpers in src/shared/lib/paths.ts that the workspace
//! needs and `monocode_core::paths` does not have yet: `parentPath`,
//! `rebasePath`, `isEqualOrInside`, `joinPath`, `setHomeDir`,
//! `resolveWorkspacePath`, `resolveWorkspaceFileReference`, and
//! `looksLikeFilePath`. Also `normalizeEditorPath` and `editorPathsEqual`
//! from src/features/search/model/search.ts, and `looksLikeProject` and
//! `isLocalProject` from src/features/projects/model/recents.ts.

use std::sync::RwLock;

use monocode_core::js;
use monocode_core::paths::{path_key, slash};
use monocode_layout::paths::{is_remote_project_path, pretty_cwd};

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
pub(crate) fn trim_slash(path: &str) -> String {
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

/// `isEqualOrInside`.
pub fn is_equal_or_inside(path: &str, root: &str) -> bool {
    let key = path_key(&trim_slash(path));
    let base_key = path_key(&trim_slash(root));
    key == base_key || key.starts_with(&format!("{base_key}/"))
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
    let bytes = &value.as_bytes()[dot + 1..];
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

/// A position inside a file reference, `:12:4` or `#L12`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileNavigation {
    pub line: i64,
    pub column: Option<i64>,
}

/// What `resolveWorkspaceFileReference` returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceFileReference {
    pub path: String,
    pub navigation: Option<FileNavigation>,
}

/// Leading digits of `value`, and the rest.
fn split_digits(value: &str) -> (&str, &str) {
    let end = value
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(value.len());
    value.split_at(end)
}

/// One full match of `(?::(\d+)(?::(\d+))?|#L(\d+)(?:-L\d+)?)` against all
/// of `rest`: the line digits and the column digits.
fn location_at(rest: &str) -> Option<(&str, Option<&str>)> {
    if let Some(tail) = rest.strip_prefix(':') {
        let (line, after) = split_digits(tail);
        if line.is_empty() {
            return None;
        }
        if after.is_empty() {
            return Some((line, None));
        }
        let column_tail = after.strip_prefix(':')?;
        let (column, end) = split_digits(column_tail);
        return (!column.is_empty() && end.is_empty()).then_some((line, Some(column)));
    }
    let tail = rest.strip_prefix("#L")?;
    let (line, after) = split_digits(tail);
    if line.is_empty() {
        return None;
    }
    if after.is_empty() {
        return Some((line, None));
    }
    let (end_line, end) = split_digits(after.strip_prefix("-L")?);
    (!end_line.is_empty() && end.is_empty()).then_some((line, None))
}

/// The leftmost location suffix, as the regex with `$` finds it: its start
/// index, its line digits, and its column digits.
fn location_suffix(value: &str) -> Option<(usize, &str, Option<&str>)> {
    value
        .char_indices()
        .filter(|(_, c)| *c == ':' || *c == '#')
        .find_map(|(index, _)| {
            location_at(&value[index..]).map(|(line, column)| (index, line, column))
        })
}

/// `Number(digits)` when it is a safe positive integer.
fn safe_positive(digits: &str) -> Option<i64> {
    const MAX_SAFE: f64 = 9_007_199_254_740_991.0;
    let number: f64 = digits.parse().ok()?;
    (number > 0.0 && number <= MAX_SAFE).then_some(number as i64)
}

/// `/^L\d+(?:-L\d+)?$/`.
fn is_line_anchor(value: &str) -> bool {
    let Some(tail) = value.strip_prefix('L') else {
        return false;
    };
    let (line, after) = split_digits(tail);
    if line.is_empty() {
        return false;
    }
    if after.is_empty() {
        return true;
    }
    let Some(end) = after.strip_prefix("-L") else {
        return false;
    };
    !end.is_empty() && end.bytes().all(|b| b.is_ascii_digit())
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

/// `parseWorkspaceFileReference`.
fn parse_workspace_file_reference(
    href: &str,
    cwd: Option<&str>,
    decode_url: bool,
) -> Option<WorkspaceFileReference> {
    let mut value = js::trim(href).to_string();
    if value.is_empty() {
        return None;
    }

    // Strip heading anchors before decoding, keeping encoded '#' in filenames.
    if decode_url
        && let Some(hash) = value.find('#')
        && hash > 0
        && !is_line_anchor(&value[hash + 1..])
    {
        value.truncate(hash);
    }
    let mut navigation = None;
    if let Some((index, line, column)) = location_suffix(&value) {
        navigation = safe_positive(line).map(|line| FileNavigation {
            line,
            column: column.and_then(safe_positive),
        });
        value.truncate(index);
    }

    let file_url = value.starts_with("file://");
    if file_url {
        value = value["file://".len()..].to_string();
        if value.starts_with("localhost/") {
            value = value["localhost".len()..].to_string();
        }
    }
    if (decode_url || file_url)
        && let Some(decoded) = decode_uri_component(&value)
    {
        // A literal percent sign is valid in a local filename.
        value = decoded;
    }

    value = slash(&value);
    let remote = cwd.and_then(remote_root);
    let found = |path: String| Some(WorkspaceFileReference { path, navigation });
    if let Some(root) = remote
        && value.starts_with(root)
    {
        return found(value);
    }
    // File URLs can also decode to UNC paths. Windows accepts mixed separators.
    if decode_url || file_url {
        let bytes = value.as_bytes();
        if bytes.len() >= 2 && matches!(bytes[0], b'/' | b'\\') && matches!(bytes[1], b'/' | b'\\')
        {
            return None;
        }
    }
    // A provider-relative `~/` reference means the user's home directory,
    // not a path relative to the project's cwd.
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
        return found(value);
    }
    // A bare filename's :line[:column] suffix must be removed before this check.
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
        return found(match remote {
            Some(root) => format!("{root}{value}"),
            None => value,
        });
    }
    if let Some(root) = remote
        && value.starts_with("//")
    {
        return found(format!("{root}{}", &value[1..]));
    }
    if let Some(rest) = value.strip_prefix('/') {
        return found(match remote {
            Some(root) => format!("{root}{}", value.trim_start_matches('/')),
            None if starts_with_drive_slash(rest) => rest.to_string(),
            None => value,
        });
    }
    let cwd = cwd.filter(|cwd| *cwd != "~" && !cwd.is_empty())?;
    found(join_path(cwd, &value))
}

/// `resolveWorkspacePath`: the absolute path for a workspace file href,
/// local or on a connected machine.
pub fn resolve_workspace_path(href: &str, cwd: Option<&str>) -> Option<String> {
    parse_workspace_file_reference(href, cwd, false).map(|reference| reference.path)
}

/// `resolveWorkspaceFileReference`: like `resolve_workspace_path`, but it
/// decodes URL escapes and keeps the source position.
pub fn resolve_workspace_file_reference(
    href: &str,
    cwd: Option<&str>,
) -> Option<WorkspaceFileReference> {
    parse_workspace_file_reference(href, cwd, true)
}

/// `decodeURIComponent`. `None` where JavaScript throws a URIError.
pub fn decode_uri_component(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            out.push(bytes[index]);
            index += 1;
            continue;
        }
        // A run of escapes must decode to whole UTF-8 characters.
        let mut run = Vec::new();
        while index < bytes.len() && bytes[index] == b'%' {
            let hex = bytes.get(index + 1..index + 3)?;
            let hex = std::str::from_utf8(hex).ok()?;
            run.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        }
        out.extend_from_slice(std::str::from_utf8(&run).ok()?.as_bytes());
    }
    String::from_utf8(out).ok()
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
    fn resolves_relative_paths_against_the_project() {
        assert_eq!(
            resolve_workspace_path("src/App.tsx", Some("/repo")).as_deref(),
            Some("/repo/src/App.tsx")
        );
        assert_eq!(
            resolve_workspace_path("src/main.ts:12:4", Some("/repo")).as_deref(),
            Some("/repo/src/main.ts")
        );
        assert_eq!(
            resolve_workspace_path("src/main.ts#L3-L9", Some("/repo")).as_deref(),
            Some("/repo/src/main.ts")
        );
        assert_eq!(resolve_workspace_path("App.tsx", Some("~")), None);
        assert_eq!(
            resolve_workspace_path("https://example.com/a.ts", Some("/repo")),
            None
        );
        assert_eq!(
            resolve_workspace_path("dependency versions", Some("/repo")),
            None
        );
    }

    #[test]
    fn keeps_the_source_position_of_a_markdown_link() {
        assert_eq!(
            resolve_workspace_file_reference("src/a%20b.ts:12:4", Some("/repo")),
            Some(WorkspaceFileReference {
                path: "/repo/src/a b.ts".into(),
                navigation: Some(FileNavigation {
                    line: 12,
                    column: Some(4)
                }),
            })
        );
        assert_eq!(
            resolve_workspace_file_reference("docs/a.md#usage", Some("/repo")),
            Some(WorkspaceFileReference {
                path: "/repo/docs/a.md".into(),
                navigation: None,
            })
        );
        assert_eq!(
            resolve_workspace_file_reference("docs/a.md#L7", Some("/repo"))
                .and_then(|reference| reference.navigation),
            Some(FileNavigation {
                line: 7,
                column: None
            })
        );
        assert_eq!(
            resolve_workspace_file_reference("//server/share/a.ts", None),
            None
        );
    }

    #[test]
    fn rebases_and_compares_paths() {
        assert_eq!(rebase_path("/a/b/c.ts", "/a/b", "/x"), "/x/c.ts");
        assert_eq!(rebase_path("/a/b", "/a/b", "C:"), "C:/");
        assert_eq!(rebase_path("/other/c.ts", "/a/b", "/x"), "/other/c.ts");
        assert!(is_equal_or_inside("/a/b/c", "/a/b"));
        assert!(is_equal_or_inside("/a/b/", "/a/b"));
        assert!(!is_equal_or_inside("/a/bc", "/a/b"));
        assert_eq!(parent_path("/a/b"), "/a");
        assert_eq!(parent_path("/a"), "/");
        assert_eq!(parent_path("C:/a"), "C:/");
        assert_eq!(join_path("/a/b", "../c/./d"), "/a/c/d");
    }

    #[test]
    fn decides_what_counts_as_a_project() {
        assert!(looks_like_project("/Users/me/repo"));
        assert!(!looks_like_project("/Users/me"));
        assert!(!looks_like_project("~"));
        assert!(!looks_like_project("/"));
        assert!(!looks_like_project("C:"));
        assert!(!looks_like_project("/Applications/X.app/Contents"));
        assert!(!is_local_project("remote://box/home/me/repo"));
        assert_eq!(normalize_editor_path("C:\\a\\b\\"), "C:/a/b");
        assert_eq!(normalize_editor_path("/"), "/");
        assert!(editor_paths_equal("C:\\A\\b", "c:/a/B"));
    }
}
