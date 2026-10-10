//! Mono-owned skills and references to existing skill folders. References keep
//! supporting files and scripts in their original location.
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::mono::{agent_dir, content_hash, monos_root, CONFLICT, FILES_LOCK};
use crate::skills::{is_discovered_skill_path, is_valid_skill_name, parse_frontmatter, scan_root};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonoSkill {
    pub name: String,
    pub description: String,
    pub path: String,
    pub hash: String,
    pub owned: bool,
    pub available: bool,
}

#[derive(Debug, Serialize)]
pub struct MonoSkillFile {
    pub text: String,
    pub hash: String,
    pub path: String,
    pub owned: bool,
}

#[derive(Deserialize)]
struct Assignment {
    name: String,
    description: String,
    path: String,
}

fn assignments(text: &str) -> Result<Vec<Assignment>, String> {
    let items: Vec<Assignment> = serde_json::from_str(text)
        .map_err(|_| "Could not read this Mono's skill assignments".to_string())?;
    if items.len() > 200 {
        return Err("A Mono can have at most 200 assigned skills".into());
    }
    let mut names = HashSet::new();
    for item in &items {
        let path = Path::new(&item.path);
        if item.name.is_empty()
            || item.name.len() > 128
            || !names.insert(&item.name)
            || !path.is_absolute()
            || !matches!(
                path.file_name().and_then(|n| n.to_str()),
                Some("SKILL.md" | "skill.md")
            )
            || !is_discovered_skill_path(&item.path)
            || item.description.len() > 4096
        {
            return Err("Invalid skill assignment".into());
        }
    }
    Ok(items)
}

pub(crate) fn validate_assignments(text: &str) -> Result<(), String> {
    assignments(text).map(|_| ())
}

pub(crate) fn owned_path(dir: &Path, name: &str) -> Result<PathBuf, String> {
    if !is_valid_skill_name(name) {
        return Err(
            "Skill names must use lowercase letters, numbers and hyphens (up to 64 characters)"
                .into(),
        );
    }
    let path = dir.join("skills").join(name).join("SKILL.md");
    // User-controlled skill names and existing symlinks must not escape a Mono.
    for entry in [
        dir.join("skills"),
        dir.join("skills").join(name),
        path.clone(),
    ] {
        match std::fs::symlink_metadata(&entry) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err("Mono skills cannot be symbolic links".into())
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(path)
}

pub(crate) fn validate_owned(path: &Path, text: &str) -> Result<(), String> {
    let folder = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|n| n.to_str())
        .ok_or("Invalid skill path")?;
    let (name, description) = parse_frontmatter(text, "");
    if name != folder || description.trim().is_empty() || description.len() > 1024 {
        return Err(
            "SKILL.md needs a matching name and a description under 1024 bytes in its frontmatter"
                .into(),
        );
    }
    Ok(())
}

fn skill_text(path: &Path) -> Result<String, String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut text = String::new();
    file.take(256 * 1024 + 1)
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    if text.len() > 256 * 1024 {
        return Err("Skill is too large".into());
    }
    Ok(text)
}

pub(crate) fn list(dir: &Path) -> Result<Vec<MonoSkill>, String> {
    let mut out = Vec::new();
    let mut names = HashSet::new();
    for skill in scan_root(&dir.join("skills"), "mono", "monocode") {
        let Ok(path) = owned_path(dir, &skill.name) else {
            continue;
        };
        if crate::fs::path_to_js(&path) != skill.path {
            continue;
        }
        let Ok(text) = skill_text(&path) else {
            continue;
        };
        names.insert(skill.name.clone());
        out.push(MonoSkill {
            name: skill.name,
            description: skill.description,
            path: skill.path,
            hash: content_hash(&text),
            owned: true,
            available: true,
        });
    }
    let assigned = match std::fs::read_to_string(dir.join("skills.json")) {
        Ok(text) => assignments(&text)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e.to_string()),
    };
    for item in assigned {
        if !names.insert(item.name.clone()) {
            continue;
        }
        let text = if is_discovered_skill_path(&item.path) {
            skill_text(Path::new(&item.path)).ok()
        } else {
            None
        };
        let description = text
            .as_ref()
            .map(|text| parse_frontmatter(text, &item.name).1)
            .filter(|description| !description.is_empty())
            .unwrap_or(item.description);
        out.push(MonoSkill {
            name: item.name,
            description,
            path: item.path,
            hash: text.as_deref().map(content_hash).unwrap_or_default(),
            owned: false,
            available: text.is_some(),
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

fn read_skill(dir: &Path, name: &str) -> Result<MonoSkillFile, String> {
    let skill = list(dir)?
        .into_iter()
        .find(|skill| skill.name == name)
        .ok_or("No skill with that name is assigned to this Mono")?;
    if !skill.owned && !is_discovered_skill_path(&skill.path) {
        return Err("This assigned skill is not in a known skill folder".into());
    }
    let text = skill_text(Path::new(&skill.path))?;
    Ok(MonoSkillFile {
        hash: content_hash(&text),
        text,
        path: skill.path,
        owned: skill.owned,
    })
}

#[tauri::command]
pub async fn mono_skill_read(
    app: AppHandle,
    mono: String,
    legacy_project: Option<String>,
    name: String,
) -> Result<MonoSkillFile, String> {
    let root = monos_root(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = FILES_LOCK.lock().map_err(|e| e.to_string())?;
        let dir = agent_dir(&root, &mono, legacy_project.as_deref())?;
        read_skill(&dir, &name)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn remove_skill(dir: &Path, name: &str, expected_hash: &str) -> Result<(), String> {
    let path = owned_path(dir, name)?;
    let text = skill_text(&path)?;
    if content_hash(&text) != expected_hash {
        return Err(CONFLICT.into());
    }
    // Only remove the definition; preserve any supporting files the user added.
    std::fs::remove_file(path).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn mono_skill_remove(
    app: AppHandle,
    mono: String,
    name: String,
    expected_hash: String,
) -> Result<(), String> {
    let root = monos_root(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = FILES_LOCK.lock().map_err(|e| e.to_string())?;
        let dir = agent_dir(&root, &mono, None)?;
        remove_skill(&dir, &name, &expected_hash)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_only_registered_skills_and_refuses_stale_deletions() {
        let dir = std::env::temp_dir().join(format!("mono-skills-{}", uuid::Uuid::new_v4()));
        let path = owned_path(&dir, "review-pr").unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let text = "---\nname: review-pr\ndescription: Review PRs\n---\n\nCheck the diff.\n";
        std::fs::write(&path, text).unwrap();
        assert!(read_skill(&dir, "unassigned").is_err());
        assert_eq!(read_skill(&dir, "review-pr").unwrap().text, text);
        assert_eq!(
            remove_skill(&dir, "review-pr", "stale"),
            Err(CONFLICT.into())
        );
        assert!(path.exists());
        remove_skill(&dir, "review-pr", &content_hash(text)).unwrap();
        assert!(list(&dir).unwrap().is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn refuses_to_read_assigned_paths_outside_discovery_roots() {
        let dir = std::env::temp_dir().join(format!("mono-skills-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let secret = dir.join("outside/SKILL.md");
        std::fs::create_dir_all(secret.parent().unwrap()).unwrap();
        std::fs::write(
            &secret,
            "---\nname: secret\ndescription: Should not be read\n---\nSECRET\n",
        )
        .unwrap();
        let allowed = dir.join("app/.agents/skills/shared");
        std::fs::create_dir_all(&allowed).unwrap();
        let allowed_path = allowed.join("SKILL.md");
        std::fs::write(
            &allowed_path,
            "---\nname: shared\ndescription: Shared workflow\n---\nFollow this.\n",
        )
        .unwrap();
        let config = serde_json::json!([
            {
                "name": "secret",
                "description": "Should not be read",
                "path": crate::fs::path_to_js(&secret)
            },
            {
                "name": "shared",
                "description": "Shared workflow",
                "path": crate::fs::path_to_js(&allowed_path)
            }
        ])
        .to_string();
        assert!(assignments(&config).is_err());
        std::fs::write(dir.join("skills.json"), config).unwrap();
        let listed = list(&dir).unwrap();
        let secret_skill = listed.iter().find(|skill| skill.name == "secret").unwrap();
        assert!(!secret_skill.available);
        assert_eq!(secret_skill.hash, "");
        assert!(read_skill(&dir, "secret").is_err());
        assert_eq!(
            read_skill(&dir, "shared").unwrap().path,
            crate::fs::path_to_js(&allowed_path)
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlinks_outside_a_monos_skills() {
        let dir = std::env::temp_dir().join(format!("mono-skills-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::os::unix::fs::symlink(std::env::temp_dir(), dir.join("skills")).unwrap();
        assert!(owned_path(&dir, "test").is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
