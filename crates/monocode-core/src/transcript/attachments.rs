//! Attachment manifests travel beside prompt text. The desktop representation
//! stays on the original user message and needs no database schema change.
use super::{Row, Transcript};
use std::path::{Path, PathBuf};

pub const ROLE: &str = "user-attachments";

pub fn from_paths(paths: &[PathBuf]) -> serde_json::Value {
    serde_json::Value::Array(
        paths
            .iter()
            .map(|path| {
                let mime = crate::composer::image_mime(path);
                serde_json::json!({
                    "id":path.to_string_lossy(), "path":path,
                    "name":path.file_name().unwrap_or_default().to_string_lossy(),
                    "mimeType":mime.unwrap_or("application/octet-stream"),
                    "kind":if mime.is_some() { "image" } else { "file" }
                })
            })
            .collect(),
    )
}

pub fn paths(row: &Row) -> Result<Vec<PathBuf>, String> {
    if row.role != ROLE {
        return Ok(Vec::new());
    }
    let files: Vec<serde_json::Value> =
        serde_json::from_str(&row.text).map_err(|e| e.to_string())?;
    files
        .iter()
        .map(|file| {
            file["path"]
                .as_str()
                .filter(|path| !path.is_empty())
                .map(PathBuf::from)
                .ok_or_else(|| "An original attachment cannot be restored safely".to_owned())
        })
        .collect()
}

/// Older native prompts kept their attachments as trailing local file links.
/// Keep those histories readable without rewriting them.
pub fn legacy_tail(text: &str) -> (&str, Vec<PathBuf>) {
    legacy_tail_with(text, |name, path| {
        let path = Path::new(path);
        (path.is_absolute() && path.file_name().and_then(|part| part.to_str()) == Some(name))
            .then(|| path.to_path_buf())
    })
}

/// Recognize foreign path syntax for a names-only export without offering
/// foreign paths to the native attachment renderer.
pub(crate) fn legacy_export_names(text: &str) -> (&str, Vec<&str>) {
    legacy_tail_with(text, |name, path| {
        ((path.starts_with('/') || is_absolute_windows_path(path))
            && export_basename(path) == Some(name))
        .then_some(name)
    })
}

fn is_absolute_windows_path(path: &str) -> bool {
    path.starts_with("\\\\")
        || matches!(path.as_bytes(), [drive, b':', b'\\' | b'/', ..] if drive.is_ascii_alphabetic())
}

pub(crate) fn export_basename(path: &str) -> Option<&str> {
    let name = if is_absolute_windows_path(path) {
        path.rsplit(['/', '\\']).next()
    } else {
        path.rsplit('/').next()
    };
    name.filter(|part| !part.trim().is_empty())
}

fn legacy_tail_with<'a, T>(
    text: &'a str,
    attachment: impl Fn(&'a str, &'a str) -> Option<T>,
) -> (&'a str, Vec<T>) {
    let mut body = text;
    let mut attachments = Vec::new();
    while let Some((before, tail)) = body.rsplit_once("\n\n") {
        let Some((name, path)) = tail
            .strip_prefix('[')
            .and_then(|tail| tail.split_once("](<"))
            .and_then(|(name, path)| path.strip_suffix(">)").map(|path| (name, path)))
        else {
            break;
        };
        let Some(attachment) = attachment(name, path) else {
            break;
        };
        attachments.push(attachment);
        body = before;
    }
    attachments.reverse();
    (body, attachments)
}

impl Transcript {
    pub fn attach_files(&mut self, paths: &[PathBuf]) {
        if !paths.is_empty() {
            let mut files = from_paths(paths);
            // Runs with provider preparation, off the UI thread.
            for (file, path) in files.as_array_mut().unwrap().iter_mut().zip(paths) {
                if let Ok(metadata) = path.metadata() {
                    file["size"] = serde_json::json!(metadata.len());
                }
            }
            self.rows.push(Row {
                role: ROLE.into(),
                text: files.to_string(),
                first: true,
                sent_at: None,
            });
        }
    }
}

#[cfg(all(test, not(windows)))]
#[test]
fn foreign_windows_legacy_names_do_not_become_native_attachment_paths() {
    let text = "A report\n\n[report.png](<C:\\workspace\\report.png>)";
    assert_eq!(legacy_export_names(text), ("A report", vec!["report.png"]));
    assert_eq!(legacy_tail(text), (text, Vec::new()));
}
