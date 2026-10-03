//! Port of host/browse.ts: lists host folders for the project picker without
//! reading any file contents.

use std::path::{Component, Path, PathBuf};

use monocode_remote::host::protocol::{HostDirectory, HostDirectoryEntry};
use serde_json::Value;

/// `path.resolve` for an absolute path: drops `.` and folds `..` without
/// touching the file system.
pub(crate) fn lexical_resolve(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => out.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(part) => out.push(part),
        }
    }
    out
}

/// `browseHostDirectories`.
pub fn browse_host_directories(raw_path: Option<&Value>) -> Result<HostDirectory, String> {
    let requested = match raw_path {
        None => None,
        Some(Value::String(text)) if text.len() <= 4096 && !text.contains('\0') => {
            Some(text.as_str()).filter(|text| !monocode_core::js::trim(text).is_empty())
        }
        Some(_) => return Err("Invalid directory path".into()),
    };
    let requested = requested
        .map(PathBuf::from)
        .unwrap_or_else(monocode_remote::host::server::home_dir);
    if !requested.is_absolute() {
        return Err("Choose an absolute directory path".into());
    }
    let path = lexical_resolve(&requested);
    let metadata = std::fs::metadata(&path).map_err(|error| error.to_string())?;
    if !metadata.is_dir() {
        return Err("Path is not a directory".into());
    }
    let mut entries: Vec<HostDirectoryEntry> = std::fs::read_dir(&path)
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| HostDirectoryEntry {
            name: entry.file_name().to_string_lossy().into_owned(),
            path: path.join(entry.file_name()).to_string_lossy().into_owned(),
        })
        .collect();
    // TODO(port): TypeScript sorted with `localeCompare`; this compares code
    // points.
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries.truncate(500);
    let parent = path
        .parent()
        .map(|parent| parent.to_string_lossy().into_owned());
    Ok(HostDirectory {
        path: path.to_string_lossy().into_owned(),
        parent,
        entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// browse.test.ts: "lists folders without exposing files and rejects
    /// relative paths".
    #[test]
    fn lists_folders_without_exposing_files_and_rejects_relative_paths() {
        let root = tempfile::tempdir().unwrap();
        let root_path = root.path().to_string_lossy().into_owned();
        std::fs::create_dir(root.path().join("repo")).unwrap();
        std::fs::write(root.path().join("secret.txt"), "private data").unwrap();
        let result = browse_host_directories(Some(&json!(root_path))).unwrap();
        assert_eq!(result.path, root_path);
        assert_eq!(
            result.entries,
            vec![HostDirectoryEntry {
                name: "repo".into(),
                path: root.path().join("repo").to_string_lossy().into_owned(),
            }]
        );
        assert!(result.parent.is_some());
        assert!(
            browse_host_directories(Some(&json!("relative/path")))
                .unwrap_err()
                .contains("absolute")
        );
        assert!(
            browse_host_directories(Some(&json!("bad\0path")))
                .unwrap_err()
                .contains("Invalid")
        );
    }

    #[test]
    fn the_root_has_no_parent() {
        let result = browse_host_directories(Some(&json!("/"))).unwrap();
        assert_eq!(result.parent, None);
    }
}
