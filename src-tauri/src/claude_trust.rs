//! The folder trust Claude Code records in `~/.claude.json`.
//!
//! An interactive `claude` in a folder it has not been told to trust opens on
//! a "Quick safety check" dialog and does nothing else until a human answers
//! it — which for a handed-over session meant a terminal raised inside
//! MonoCode, a cursor sitting on "No, exit", and a phone that could not see
//! the conversation until it was dealt with. The headless path never asks,
//! so the folder is one MonoCode is already working in.
//!
//! The answer lives at `projects["<folder>"].hasTrustDialogAccepted`, and
//! writing it ahead of the hand-over is the same outcome as answering the
//! dialog, minus the dialog. The decision stays with the user: MonoCode asks
//! once, in its own UI, and only writes on a yes.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::dirs_home;

fn config_path() -> Result<PathBuf, String> {
    dirs_home()
        .map(|home| PathBuf::from(home).join(".claude.json"))
        .ok_or_else(|| "Could not find the home directory".to_string())
}

fn read_config(path: &Path) -> Result<Value, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|e| format!("{} is not valid JSON: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Value::Object(Map::new())),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// The key Claude Code files a folder under: the path as given, without a
/// trailing separator, which is how the entries it writes itself look.
fn project_key(folder: &str) -> String {
    let trimmed = folder.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        folder.to_string()
    } else {
        trimmed.to_string()
    }
}

pub fn is_trusted(config: &Value, folder: &str) -> bool {
    config
        .get("projects")
        .and_then(|projects| projects.get(project_key(folder)))
        .and_then(|project| project.get("hasTrustDialogAccepted"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// Mark the folder trusted, keeping everything else in the document as it was.
pub fn mark_trusted(config: &mut Value, folder: &str) {
    if !config.is_object() {
        *config = Value::Object(Map::new());
    }
    let root = config.as_object_mut().expect("made an object above");
    let projects = root
        .entry("projects")
        .or_insert_with(|| Value::Object(Map::new()));
    if !projects.is_object() {
        *projects = Value::Object(Map::new());
    }
    let project = projects
        .as_object_mut()
        .expect("made an object above")
        .entry(project_key(folder))
        .or_insert_with(|| Value::Object(Map::new()));
    if !project.is_object() {
        *project = Value::Object(Map::new());
    }
    project
        .as_object_mut()
        .expect("made an object above")
        .insert("hasTrustDialogAccepted".into(), Value::Bool(true));
}

/// Whole-file replace through a sibling temp file, so a reader never sees a
/// half-written document. Claude Code rewrites this file itself, so the
/// window for a lost update exists either way; a lost update here costs one
/// more dialog, which the terminal path still handles.
fn write_config(path: &Path, config: &Value) -> Result<(), String> {
    let text = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.monocode-tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Whether Claude Code would open on the trust dialog for this folder.
#[tauri::command]
pub async fn claude_folder_trusted(folder: String) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = config_path()?;
        Ok(is_trusted(&read_config(&path)?, &folder))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Record the folder as trusted, as answering the dialog would.
#[tauri::command]
pub async fn claude_trust_folder(folder: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = config_path()?;
        let mut config = read_config(&path)?;
        if is_trusted(&config, &folder) {
            return Ok(());
        }
        mark_trusted(&mut config, &folder);
        write_config(&path, &config)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_the_flag_claude_code_writes() {
        let config = json!({
            "projects": {
                "/Users/x/Projects/test": { "hasTrustDialogAccepted": true, "allowedTools": [] },
                "/Users/x/Projects/other": { "allowedTools": [] }
            }
        });
        assert!(is_trusted(&config, "/Users/x/Projects/test"));
        assert!(is_trusted(&config, "/Users/x/Projects/test/"));
        assert!(!is_trusted(&config, "/Users/x/Projects/other"));
        assert!(!is_trusted(&config, "/Users/x/Projects/new"));
        assert!(!is_trusted(&json!({}), "/Users/x/Projects/test"));
    }

    #[test]
    fn marks_a_folder_without_touching_the_rest() {
        let mut config = json!({
            "oauthAccount": { "emailAddress": "a@b" },
            "projects": {
                "/Users/x/Projects/other": { "allowedTools": ["Bash"] }
            }
        });
        mark_trusted(&mut config, "/Users/x/Projects/new/");
        assert!(is_trusted(&config, "/Users/x/Projects/new"));
        assert_eq!(config["oauthAccount"]["emailAddress"], "a@b");
        assert_eq!(
            config["projects"]["/Users/x/Projects/other"]["allowedTools"][0],
            "Bash"
        );
        // The key is the path without its trailing separator, as Claude Code
        // files it.
        assert!(config["projects"].get("/Users/x/Projects/new/").is_none());
    }

    #[test]
    fn keeps_an_existing_project_entry() {
        let mut config = json!({
            "projects": { "/p": { "allowedTools": ["Read"], "lastCost": 1.5 } }
        });
        mark_trusted(&mut config, "/p");
        assert_eq!(config["projects"]["/p"]["allowedTools"][0], "Read");
        assert_eq!(config["projects"]["/p"]["lastCost"], 1.5);
        assert_eq!(config["projects"]["/p"]["hasTrustDialogAccepted"], true);
    }

    #[test]
    fn starts_from_nothing() {
        let mut config = Value::Null;
        mark_trusted(&mut config, "/p");
        assert!(is_trusted(&config, "/p"));
    }
}
