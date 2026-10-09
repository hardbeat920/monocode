//! Supervised Devin sessions. Devin's ACP server has no mode that asks before
//! workspace edits, but its permission rules can: the child runs with a copy of
//! the user's config whose `ask` list also covers every write. `--config`
//! replaces the user config rather than layering on it, so the copy keeps the
//! user's own settings.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::dirs_home;

pub(crate) const ASK_EDITS_RULE: &str = "Write(**)";

/// The `--config` arguments for a supervised `devin acp` child.
pub(crate) fn supervised_args(app: &AppHandle) -> Result<Vec<String>, String> {
    let config = ask_before_edits(read_user_config()?.as_deref())?;
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("devin");
    let path = write_once(&dir, &config)?;
    Ok(vec!["--config".into(), path.to_string_lossy().into_owned()])
}

/// The user config file Devin itself reads.
fn user_config_path() -> Option<PathBuf> {
    let env = |key: &str| {
        std::env::var_os(key)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    let dir = if cfg!(windows) {
        env("APPDATA")
    } else {
        env("XDG_CONFIG_HOME")
            .or_else(|| dirs_home().map(|home| PathBuf::from(home).join(".config")))
    }?;
    Some(dir.join("devin").join("config.json"))
}

fn read_user_config() -> Result<Option<String>, String> {
    let Some(path) = user_config_path() else {
        return Ok(None);
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("Could not read {}: {error}", path.display())),
    }
}

/// The user's config with an `ask` rule for every write. Anything it cannot
/// extend safely is an error: Supervised must not start without the rule.
pub(crate) fn ask_before_edits(user: Option<&str>) -> Result<String, String> {
    const HELP: &str = "Supervised mode needs it to make Devin ask before edits.";
    let mut config = match user.map(str::trim).filter(|text| !text.is_empty()) {
        Some(text) => serde_json::from_str::<Value>(text)
            .map_err(|e| format!("Devin's config.json is not valid JSON ({e}). {HELP}"))?,
        None => json!({}),
    };
    let ask = config
        .as_object_mut()
        .ok_or_else(|| format!("Devin's config.json is not a JSON object. {HELP}"))?
        .entry("permissions")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| format!("Devin's config.json `permissions` is not an object. {HELP}"))?
        .entry("ask")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(|| format!("Devin's config.json `permissions.ask` is not a list. {HELP}"))?;
    if !ask.iter().any(|rule| rule.as_str() == Some(ASK_EDITS_RULE)) {
        ask.push(json!(ASK_EDITS_RULE));
    }
    serde_json::to_string_pretty(&config).map_err(|e| e.to_string())
}

/// Named by content so a running child's file is never rewritten in place.
fn write_once(dir: &Path, contents: &str) -> Result<PathBuf, String> {
    let mut hasher = DefaultHasher::new();
    contents.hash(&mut hasher);
    let path = dir.join(format!("supervised-{:016x}.json", hasher.finish()));
    if path.is_file() {
        return Ok(path);
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    let temp = dir.join(format!(".{}-{stamp}.tmp", std::process::id()));
    // The copy can hold whatever the user keeps in config.json: owner-only.
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(&temp)
        .and_then(|mut file| file.write_all(contents.as_bytes()))
        .map_err(|e| format!("{}: {e}", temp.display()))?;
    if let Err(error) = std::fs::rename(&temp, &path) {
        let _ = std::fs::remove_file(&temp);
        // Another spawn wrote the same content first.
        if !path.is_file() {
            return Err(format!("{}: {error}", path.display()));
        }
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask_rules(config: &str) -> Vec<String> {
        let value: Value = serde_json::from_str(config).unwrap();
        value["permissions"]["ask"]
            .as_array()
            .unwrap()
            .iter()
            .map(|rule| rule.as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn adds_the_write_rule_and_keeps_the_users_settings() {
        let merged = ask_before_edits(Some(
            r#"{"theme_mode":"dark","permissions":{"allow":["Exec(python3)"],"ask":["exec"]}}"#,
        ))
        .unwrap();
        let value: Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(value["theme_mode"], "dark");
        assert_eq!(value["permissions"]["allow"], json!(["Exec(python3)"]));
        assert_eq!(ask_rules(&merged), vec!["exec", ASK_EDITS_RULE]);
        // Idempotent: the rule is not added twice.
        assert_eq!(
            ask_rules(&ask_before_edits(Some(&merged)).unwrap()),
            vec!["exec", ASK_EDITS_RULE]
        );
    }

    #[test]
    fn starts_from_an_empty_config() {
        assert_eq!(
            ask_rules(&ask_before_edits(None).unwrap()),
            vec![ASK_EDITS_RULE]
        );
        assert_eq!(
            ask_rules(&ask_before_edits(Some("  ")).unwrap()),
            vec![ASK_EDITS_RULE]
        );
    }

    #[test]
    fn refuses_configs_it_cannot_extend() {
        assert!(ask_before_edits(Some("{ // comment\n}")).is_err());
        assert!(ask_before_edits(Some("[]")).is_err());
        assert!(ask_before_edits(Some(r#"{"permissions":[]}"#)).is_err());
        assert!(ask_before_edits(Some(r#"{"permissions":{"ask":"Write(**)"}}"#)).is_err());
    }

    #[test]
    fn writes_each_config_once_by_content() {
        let dir =
            std::env::temp_dir().join(format!("monocode-devin-config-{}", std::process::id()));
        let first = write_once(&dir, "{}").unwrap();
        assert_eq!(write_once(&dir, "{}").unwrap(), first);
        assert_eq!(std::fs::read_to_string(&first).unwrap(), "{}");
        assert_ne!(write_once(&dir, "{\"a\":1}").unwrap(), first);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
