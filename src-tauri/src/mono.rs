//! Each Mono keeps its files in the app's data folder, never in a repo: it
//! works in repos with full file access, so a soul kept there could be
//! rewritten outside the app's controlled CLI or by a pull request.
//!
//! ```text
//! <app data>/monos/
//! ├── index.json            from when each project had its own Mono:
//! │                         folder id → the project keys it answered for
//! └── <mono id>/
//!     ├── SOUL.md           who it is; edited by the user or at their request
//!     ├── MEMORY.md         what it has learned; loads into every turn
//!     ├── habits.json       what it does on its own, on a schedule
//!     ├── skills.json       references to skills assigned by the user
//!     ├── skills/           skills created just for this Mono
//!     └── memory/           topic notes read on demand, and archive.md
//! ```
//!
//! A Mono is not tied to a project, so its folder is named by its own id. One
//! that was a project's Mono passes that project along until its old folder
//! has moved under its id.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

const MONOS_DIR: &str = "monos";
/// Where Monos lived before they had their name; moved on first use.
const LEGACY_DIR: &str = "agents";
const INDEX_FILE: &str = "index.json";
const SOUL_FILE: &str = "SOUL.md";
const MEMORY_FILE: &str = "MEMORY.md";
const MEMORY_DIR: &str = "memory";
const HABITS_FILE: &str = "habits.json";
/// Larger than anything that would load: the soul and memory both have far
/// smaller prompt budgets, this only stops a runaway write.
const FILE_MAX: usize = 256 * 1024;
pub const CONFLICT: &str = "conflict";

/// Serializes index updates, so two Monos opening at once cannot both read
/// the old index and drop each other's entry.
static INDEX_LOCK: Mutex<()> = Mutex::new(());

/// All windows share this store. Keep initialization and hash-checked writes
/// serialized so only one caller can save over a version of a file.
pub(crate) static FILES_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Default, Serialize, Deserialize)]
struct AgentIndex {
    #[serde(default)]
    agents: Vec<AgentEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AgentEntry {
    id: String,
    /// Project keys (normalized paths) its project's Mono answered for.
    projects: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentFile {
    /// Absent when the file does not exist yet.
    pub text: Option<String>,
    /// Hash of the text, or of "" for a missing file.
    pub hash: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentFiles {
    pub id: String,
    pub dir: String,
    /// Absent until the app seeds it; the seed needs the user's old settings.
    pub soul: Option<String>,
    pub soul_hash: String,
    pub memory: String,
    pub memory_hash: String,
    pub memory_path: String,
    /// Topic notes under memory/, by name without `.md`; not the archive.
    pub topics: Vec<String>,
    pub skills: Vec<crate::mono_skills::MonoSkill>,
    pub skills_hash: String,
}

/// The files the app reads and writes for an agent, relative to its folder:
/// SOUL.md, MEMORY.md, habits.json, or one `memory/<name>.md`. Nothing nested, nothing
/// hidden, so a path from the agent's CLI can never leave the folder.
fn resolve_file(dir: &Path, relative: &str) -> Result<PathBuf, String> {
    if relative == SOUL_FILE || relative == MEMORY_FILE || relative == HABITS_FILE {
        return Ok(dir.join(relative));
    }
    if relative == "skills.json" {
        return Ok(dir.join(relative));
    }
    if let Some(name) = relative
        .strip_prefix("skills/")
        .and_then(|rest| rest.strip_suffix("/SKILL.md"))
    {
        return crate::mono_skills::owned_path(dir, name);
    }
    let name = relative
        .strip_prefix("memory/")
        .and_then(|rest| rest.strip_suffix(".md"))
        .ok_or_else(|| format!("Not an agent file: {relative}"))?;
    let valid = !name.is_empty()
        && name.len() <= 80
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '.' | '-' | '_'));
    if !valid {
        return Err(format!("Not an agent file: {relative}"));
    }
    Ok(dir.join(MEMORY_DIR).join(format!("{name}.md")))
}

fn topics(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir.join(MEMORY_DIR))
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.path().is_file())
                .filter_map(|entry| {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    let topic = name.strip_suffix(".md")?.to_string();
                    (topic != "archive" && !topic.starts_with('.')).then_some(topic)
                })
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

pub(crate) fn monos_root(app: &AppHandle) -> Result<PathBuf, String> {
    let data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let root = data.join(MONOS_DIR);
    let legacy = data.join(LEGACY_DIR);
    if !root.exists() && legacy.join(INDEX_FILE).is_file() {
        let _ = std::fs::rename(&legacy, &root);
    }
    Ok(root)
}

pub fn content_hash(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Write beside the target and rename over it, so a crash mid-write leaves
/// the old file whole. Owner-only, since memory can hold personal details.
fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    use std::io::Write;

    let temp = path.with_extension("md.tmp");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temp)
        .map_err(|e| format!("{}: {e}", temp.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // A temporary file left by an older version may have wider permissions.
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("{}: {e}", temp.display()))?;
    }
    file.write_all(text.as_bytes())
        .map_err(|e| format!("{}: {e}", temp.display()))?;
    drop(file);
    std::fs::rename(&temp, path).map_err(|e| format!("{}: {e}", path.display()))
}

fn read_optional(path: &Path) -> Result<Option<String>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

fn read_index(root: &Path) -> AgentIndex {
    std::fs::read_to_string(root.join(INDEX_FILE))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

/// The Mono's folder, created with an empty memory on first use. A Mono that
/// was a project's own takes over that project's folder the first time.
pub(crate) fn agent_dir(
    root: &Path,
    mono: &str,
    legacy_project: Option<&str>,
) -> Result<PathBuf, String> {
    if !valid_id(mono) {
        return Err("Not a Mono id".into());
    }
    std::fs::create_dir_all(root).map_err(|e| format!("{}: {e}", root.display()))?;
    let dir = root.join(mono);
    if let Some(project) = legacy_project.filter(|project| !project.trim().is_empty()) {
        adopt_legacy_folder(root, &dir, project)?;
    }
    std::fs::create_dir_all(dir.join(MEMORY_DIR)).map_err(|e| format!("{}: {e}", dir.display()))?;
    let memory = dir.join(MEMORY_FILE);
    if !memory.exists() {
        write_atomic(&memory, "")?;
    }
    Ok(dir)
}

/// Moves the folder the project's Mono had to `dir`, once, and drops it from
/// the index so no other Mono can take it too.
fn adopt_legacy_folder(root: &Path, dir: &Path, project: &str) -> Result<(), String> {
    let _guard = INDEX_LOCK.lock().map_err(|e| e.to_string())?;
    let mut index = read_index(root);
    let Some(position) = index
        .agents
        .iter()
        .position(|entry| valid_id(&entry.id) && entry.projects.iter().any(|key| key == project))
    else {
        return Ok(());
    };
    let entry = index.agents.remove(position);
    let legacy = root.join(&entry.id);
    if !dir.exists() && legacy.is_dir() && legacy != dir {
        std::fs::rename(&legacy, dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let text = serde_json::to_string_pretty(&index).map_err(|e| e.to_string())?;
    let path = root.join(INDEX_FILE);
    let temp = root.join("index.json.tmp");
    std::fs::write(&temp, text).map_err(|e| format!("{}: {e}", temp.display()))?;
    std::fs::rename(&temp, &path).map_err(|e| format!("{}: {e}", path.display()))
}

fn load(root: &Path, mono: &str, legacy_project: Option<&str>) -> Result<AgentFiles, String> {
    let _guard = FILES_LOCK.lock().map_err(|e| e.to_string())?;
    let dir = agent_dir(root, mono, legacy_project)?;
    let soul = read_optional(&dir.join(SOUL_FILE))?;
    let memory_path = dir.join(MEMORY_FILE);
    let memory = read_optional(&memory_path)?.unwrap_or_default();
    let skills = crate::mono_skills::list(&dir)?;
    let skills_hash = content_hash(&serde_json::to_string(&skills).map_err(|e| e.to_string())?);
    Ok(AgentFiles {
        id: mono.to_string(),
        soul_hash: content_hash(soul.as_deref().unwrap_or("")),
        soul,
        memory_hash: content_hash(&memory),
        memory,
        memory_path: memory_path.to_string_lossy().into_owned(),
        topics: topics(&dir),
        skills,
        skills_hash,
        dir: dir.to_string_lossy().into_owned(),
    })
}

fn read(
    root: &Path,
    mono: &str,
    legacy_project: Option<&str>,
    relative: &str,
) -> Result<AgentFile, String> {
    let _guard = FILES_LOCK.lock().map_err(|e| e.to_string())?;
    let dir = agent_dir(root, mono, legacy_project)?;
    let text = read_optional(&resolve_file(&dir, relative)?)?;
    Ok(AgentFile {
        hash: content_hash(text.as_deref().unwrap_or("")),
        text,
    })
}

/// Saves only over the text the editor opened, so the user never overwrites
/// what the agent wrote while they were typing. No hash means overwrite.
fn save(
    root: &Path,
    mono: &str,
    legacy_project: Option<&str>,
    relative: &str,
    text: &str,
    expected_hash: Option<&str>,
) -> Result<String, String> {
    if text.len() > FILE_MAX {
        return Err(format!("{relative} is too large"));
    }
    let _guard = FILES_LOCK.lock().map_err(|e| e.to_string())?;
    let dir = agent_dir(root, mono, legacy_project)?;
    let path = resolve_file(&dir, relative)?;
    if relative.starts_with("skills/") {
        crate::mono_skills::validate_owned(&path, text)?;
        std::fs::create_dir_all(path.parent().ok_or("Invalid skill path")?)
            .map_err(|e| e.to_string())?;
    } else if relative == "skills.json" {
        crate::mono_skills::validate_assignments(text)?;
    }
    if let Some(expected) = expected_hash {
        let current = read_optional(&path)?.unwrap_or_default();
        if content_hash(&current) != expected {
            return Err(CONFLICT.into());
        }
    }
    write_atomic(&path, text)?;
    Ok(content_hash(text))
}

#[tauri::command]
pub async fn mono_load(
    app: AppHandle,
    mono: String,
    legacy_project: Option<String>,
) -> Result<AgentFiles, String> {
    let root = monos_root(&app)?;
    tauri::async_runtime::spawn_blocking(move || load(&root, &mono, legacy_project.as_deref()))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn mono_read(
    app: AppHandle,
    mono: String,
    legacy_project: Option<String>,
    path: String,
) -> Result<AgentFile, String> {
    let root = monos_root(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        read(&root, &mono, legacy_project.as_deref(), &path)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn mono_save(
    app: AppHandle,
    mono: String,
    legacy_project: Option<String>,
    path: String,
    text: String,
    expected_hash: Option<String>,
) -> Result<String, String> {
    let root = monos_root(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        save(
            &root,
            &mono,
            legacy_project.as_deref(),
            &path,
            &text,
            expected_hash.as_deref(),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root() -> PathBuf {
        let root = std::env::temp_dir().join(format!("mono-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn creates_one_folder_per_mono_with_empty_memory() {
        let root = temp_root();
        let first = load(&root, "mono-a", None).unwrap();
        let again = load(&root, "mono-a", None).unwrap();
        let other = load(&root, "mono-b", None).unwrap();
        assert_eq!(first.id, "mono-a");
        assert_eq!(first.dir, again.dir);
        assert_ne!(first.dir, other.dir);
        assert_eq!(first.soul, None);
        assert_eq!(first.memory, "");
        assert!(Path::new(&first.dir).join("memory").is_dir());
        assert!(load(&root, "../x", None).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn takes_over_the_folder_its_project_had_once() {
        let root = temp_root();
        let old = root.join("old-id");
        std::fs::create_dir_all(old.join("memory")).unwrap();
        std::fs::write(old.join(MEMORY_FILE), "- kept").unwrap();
        std::fs::write(
            root.join(INDEX_FILE),
            r#"{"agents":[{"id":"old-id","projects":["/code/app"]}]}"#,
        )
        .unwrap();
        let adopted = load(&root, "mono-a", Some("/code/app")).unwrap();
        assert_eq!(adopted.memory, "- kept");
        assert!(!old.exists());
        // Another Mono naming the same project starts fresh.
        let other = load(&root, "mono-b", Some("/code/app")).unwrap();
        assert_eq!(other.memory, "");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn refuses_to_overwrite_a_changed_file() {
        let root = temp_root();
        let files = load(&root, "m", None).unwrap();
        let hash = save(
            &root,
            "m",
            None,
            MEMORY_FILE,
            "- a",
            Some(&files.memory_hash),
        )
        .unwrap();
        assert_eq!(
            save(
                &root,
                "m",
                None,
                MEMORY_FILE,
                "- b",
                Some(&files.memory_hash)
            ),
            Err(CONFLICT.into())
        );
        save(&root, "m", None, MEMORY_FILE, "- c", Some(&hash)).unwrap();
        save(&root, "m", None, SOUL_FILE, "soul", None).unwrap();
        let loaded = load(&root, "m", None).unwrap();
        assert_eq!(loaded.memory, "- c");
        assert_eq!(loaded.soul.as_deref(), Some("soul"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn writes_private_files_even_with_a_leftover_public_temp_file() {
        use std::os::unix::fs::PermissionsExt;

        let root = temp_root();
        let path = root.join(MEMORY_FILE);
        write_atomic(&path, "private memory").unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );

        let temp = path.with_extension("md.tmp");
        std::fs::write(&temp, "leftover").unwrap();
        std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o644)).unwrap();
        write_atomic(&path, "new private memory").unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "new private memory"
        );
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(!temp.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn concurrent_habit_claims_only_save_over_a_version_once() {
        use std::sync::{Arc, Barrier};

        let root = temp_root();
        let hash = save(&root, "m", None, HABITS_FILE, "[]", None).unwrap();
        let barrier = Arc::new(Barrier::new(8));
        let claims: Vec<_> = (0..8)
            .map(|index| {
                let root = root.clone();
                let hash = hash.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let text = format!("[{{\"claimedBy\":{index}}}]");
                    barrier.wait();
                    let result = save(&root, "m", None, HABITS_FILE, &text, Some(&hash));
                    (text, result)
                })
            })
            .collect();
        let results: Vec<_> = claims
            .into_iter()
            .map(|claim| claim.join().unwrap())
            .collect();
        let winners: Vec<_> = results
            .iter()
            .filter(|(_, result)| result.is_ok())
            .collect();
        assert_eq!(winners.len(), 1);
        for (_, result) in results.iter().filter(|(_, result)| result.is_err()) {
            assert_eq!(result.as_ref().unwrap_err(), CONFLICT);
        }
        let saved = read(&root, "m", None, HABITS_FILE).unwrap();
        assert_eq!(saved.text.as_ref(), Some(&winners[0].0));
        assert_eq!(saved.hash, content_hash(&winners[0].0));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reads_and_writes_topics_inside_the_folder_only() {
        let root = temp_root();
        save(&root, "m", None, "memory/releases.md", "# Releases", None).unwrap();
        save(&root, "m", None, "memory/archive.md", "- old", None).unwrap();
        let loaded = load(&root, "m", None).unwrap();
        assert_eq!(loaded.topics, vec!["releases".to_string()]);
        let topic = read(&root, "m", None, "memory/releases.md").unwrap();
        assert_eq!(topic.text.as_deref(), Some("# Releases"));
        assert_eq!(read(&root, "m", None, "memory/none.md").unwrap().text, None);
        save(&root, "m", None, "habits.json", "[]", None).unwrap();
        for path in [
            "../x.md",
            "memory/../SOUL.md",
            "memory/a/b.md",
            "memory/.hidden.md",
            "notes.md",
        ] {
            assert!(save(&root, "m", None, path, "x", None).is_err(), "{path}");
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn skills_are_owned_by_one_mono_and_updated_with_hashes() {
        let root = temp_root();
        let path = "skills/review-pr/SKILL.md";
        let text =
            "---\nname: review-pr\ndescription: 'Review the team''s PRs'\n---\n\nRun checks.\n";
        let empty = read(&root, "a", None, path).unwrap();
        let hash = save(&root, "a", None, path, text, Some(&empty.hash)).unwrap();
        let first = load(&root, "a", None).unwrap();
        assert_eq!(first.skills.len(), 1);
        assert_eq!(first.skills[0].description, "Review the team's PRs");
        assert!(first.skills[0].owned);
        assert!(load(&root, "b", None).unwrap().skills.is_empty());
        let updated = text.replace("Run checks.", "Run checks and inspect the diff.");
        save(&root, "a", None, path, &updated, Some(&hash)).unwrap();
        assert_ne!(
            first.skills_hash,
            load(&root, "a", None).unwrap().skills_hash
        );
        assert_eq!(
            save(&root, "a", None, path, text, Some(&hash)),
            Err(CONFLICT.into())
        );
        for invalid in [
            "skills/../SKILL.md",
            "skills/a/b/SKILL.md",
            "skills/.hidden/SKILL.md",
        ] {
            assert!(save(&root, "a", None, invalid, text, None).is_err());
        }
        assert!(save(&root, "a", None, path, "no metadata", None).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn assigned_skills_keep_sources_and_follow_source_changes() {
        let root = temp_root();
        let folder = root.join("app/.agents/skills/shared");
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("SKILL.md");
        let body = "---\nname: shared\ndescription: Shared workflow\n---\n\nFollow reference.md.\n";
        std::fs::write(&path, body).unwrap();
        std::fs::write(folder.join("reference.md"), "Supporting content").unwrap();
        let config = serde_json::json!([{
            "name": "shared",
            "description": "Shared workflow",
            "path": crate::fs::path_to_js(&path)
        }])
        .to_string();
        save(&root, "a", None, "skills.json", &config, None).unwrap();
        let first = load(&root, "a", None).unwrap();
        assert!(!first.skills[0].owned);
        assert!(first.skills[0].available);
        assert!(load(&root, "b", None).unwrap().skills.is_empty());
        std::fs::write(&path, body.replace("Shared workflow", "Updated workflow")).unwrap();
        let next = load(&root, "a", None).unwrap();
        assert_eq!(next.skills[0].description, "Updated workflow");
        assert_ne!(first.skills_hash, next.skills_hash);
        save(&root, "a", None, "skills.json", "[]", None).unwrap();
        assert!(load(&root, "a", None).unwrap().skills.is_empty());
        assert!(path.is_file());
        assert!(folder.join("reference.md").is_file());
        save(&root, "a", None, "skills.json", &config, None).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!(!load(&root, "a", None).unwrap().skills[0].available);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn assigned_skills_must_match_a_discovery_root() {
        let root = temp_root();
        let secret = root.join("secret/SKILL.md");
        std::fs::create_dir_all(secret.parent().unwrap()).unwrap();
        std::fs::write(
            &secret,
            "---\nname: secret\ndescription: Should not be read\n---\nSECRET\n",
        )
        .unwrap();
        let config = serde_json::json!([{
            "name": "secret",
            "description": "Should not be read",
            "path": crate::fs::path_to_js(&secret)
        }])
        .to_string();
        assert!(save(&root, "a", None, "skills.json", &config, None).is_err());
        let dir = agent_dir(&root, "a", None).unwrap();
        std::fs::write(dir.join("skills.json"), &config).unwrap();
        let listed = load(&root, "a", None).unwrap();
        assert_eq!(listed.skills.len(), 1);
        assert!(!listed.skills[0].available);
        assert_eq!(listed.skills[0].description, "Should not be read");
        assert_eq!(listed.skills[0].hash, "");
        std::fs::remove_dir_all(root).unwrap();
    }
}
