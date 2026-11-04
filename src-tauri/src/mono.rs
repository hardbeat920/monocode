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
//!     └── memory/           topic notes read on demand, and archive.md
//! ```
//!
//! A Mono is not tied to a project, so its folder is named by its own id. One
//! that was a project's Mono passes that project along until its old folder
//! has moved under its id.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

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
static FILES_LOCK: Mutex<()> = Mutex::new(());

#[derive(Default)]
struct SoulWrites {
    active: HashMap<String, String>,
    epoch: u64,
}

static SOUL_WRITES: LazyLock<Mutex<SoulWrites>> =
    LazyLock::new(|| Mutex::new(SoulWrites::default()));

impl SoulWrites {
    fn begin(&mut self, mono: &str, id: &str, registered: bool) -> Result<(), String> {
        let mono = mono.to_ascii_lowercase();
        if registered && id != self.epoch.to_string() {
            return Err("Soul save cancelled; retry".into());
        }
        if self.active.contains_key(&mono) {
            return Err("A previous Soul save is still in progress".into());
        }
        self.active.insert(mono, id.to_string());
        Ok(())
    }

    fn cancel(&mut self, id: &str) -> Result<(), String> {
        if id == self.epoch.to_string() {
            self.epoch = self
                .epoch
                .checked_add(1)
                .ok_or("Soul save generation exhausted")?;
        }
        Ok(())
    }

    fn finish(&mut self, mono: &str, id: &str) {
        let mono = mono.to_ascii_lowercase();
        if self.active.get(&mono).is_some_and(|active| active == id) {
            self.active.remove(&mono);
        }
    }
}

struct SoulWritePermit {
    mono: String,
    id: String,
}
impl Drop for SoulWritePermit {
    fn drop(&mut self) {
        if let Ok(mut writes) = SOUL_WRITES.lock() {
            writes.finish(&self.mono, &self.id);
        }
    }
}

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
}

/// The files the app reads and writes for an agent, relative to its folder:
/// SOUL.md, MEMORY.md, habits.json, or one `memory/<name>.md`. Nothing nested, nothing
/// hidden, so a path from the agent's CLI can never leave the folder.
fn resolve_file(dir: &Path, relative: &str) -> Result<PathBuf, String> {
    if relative == SOUL_FILE || relative == MEMORY_FILE || relative == HABITS_FILE {
        return Ok(dir.join(relative));
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

fn monos_root(app: &AppHandle) -> Result<PathBuf, String> {
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
fn agent_dir(root: &Path, mono: &str, legacy_project: Option<&str>) -> Result<PathBuf, String> {
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
    Ok(AgentFiles {
        id: mono.to_string(),
        soul_hash: content_hash(soul.as_deref().unwrap_or("")),
        soul,
        memory_hash: content_hash(&memory),
        memory,
        memory_path: memory_path.to_string_lossy().into_owned(),
        topics: topics(&dir),
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
    request_id: Option<String>,
) -> Result<String, String> {
    let root = monos_root(&app)?;
    let permit = if path == SOUL_FILE {
        if !valid_id(&mono) {
            return Err("Invalid Soul save request".into());
        }
        let registered = request_id.is_some();
        let id = request_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        SOUL_WRITES
            .lock()
            .map_err(|e| e.to_string())?
            .begin(&mono, &id, registered)?;
        Some(SoulWritePermit {
            mono: mono.clone(),
            id,
        })
    } else {
        None
    };
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
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

/// Epochs fence delayed editor requests across all windows.
#[tauri::command]
pub fn mono_register_soul_save() -> Result<String, String> {
    Ok(SOUL_WRITES
        .lock()
        .map_err(|e| e.to_string())?
        .epoch
        .to_string())
}

#[tauri::command]
pub fn mono_cancel_soul_save(mono: String, request_id: String) -> Result<(), String> {
    if !valid_id(&mono) {
        return Err("Invalid Soul save request".into());
    }
    request_id
        .parse::<u64>()
        .map_err(|_| "Invalid Soul save request")?;
    SOUL_WRITES
        .lock()
        .map_err(|e| e.to_string())?
        .cancel(&request_id)
}

/// Recovery copies never touch the Mono file lock or overwrite an existing file.
#[tauri::command]
pub async fn mono_save_soul_recovery(
    app: AppHandle,
    mono: String,
    text: String,
) -> Result<String, String> {
    if !valid_id(&mono) || text.len() > 8 * 1024 * 1024 {
        return Err("Invalid Soul recovery draft".into());
    }
    let root = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("soul-recovery");
    tauri::async_runtime::spawn_blocking(move || save_soul_recovery(&root, &mono, &text))
        .await
        .map_err(|e| e.to_string())?
}

fn save_soul_recovery(root: &Path, mono: &str, text: &str) -> Result<String, String> {
    use std::io::Write;
    std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
    if std::fs::symlink_metadata(root)
        .map_err(|e| e.to_string())?
        .file_type()
        .is_symlink()
    {
        return Err("Recovery folder cannot be a symbolic link".into());
    }
    let path = root.join(format!("{mono}-{}.md", uuid::Uuid::new_v4()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&path).map_err(|e| e.to_string())?;
    let result = file
        .write_all(text.as_bytes())
        .and_then(|_| file.sync_all());
    drop(file);
    if let Err(error) = result {
        let _ = std::fs::remove_file(&path);
        return Err(error.to_string());
    }
    #[cfg(unix)]
    {
        std::fs::File::open(root)
            .and_then(|dir| dir.sync_all())
            .map_err(|e| e.to_string())?;
        if let Some(parent) = root.parent() {
            std::fs::File::open(parent)
                .and_then(|dir| dir.sync_all())
                .map_err(|e| e.to_string())?;
        }
    }
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| "Recovery path is not valid UTF-8".into())
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
    fn soul_epochs_are_bounded_and_fence_out_of_order_requests() {
        let mut writes = SoulWrites::default();
        let old = writes.epoch.to_string();
        writes.begin("mono", &old, true).unwrap();
        writes.cancel(&old).unwrap();
        assert!(writes.begin("other-window-mono", &old, true).is_err());
        let next = writes.epoch.to_string();
        assert!(writes.begin("MONO", &next, true).is_err());
        writes.finish("mono", &old);
        assert!(writes.begin("mono", &old, true).is_err());
        writes.begin("mono", &next, true).unwrap();
        writes.finish("mono", &next);
        for _ in 0..1100 {
            writes.cancel(&writes.epoch.to_string()).unwrap();
        }
        let current = writes.epoch;
        writes.cancel(&old).unwrap();
        assert_eq!(writes.epoch, current);
        assert!(writes.begin("mono", &old, true).is_err());
        assert!(writes.begin("mono", &current.to_string(), true).is_ok());
    }

    #[test]
    fn recovery_copies_are_distinct_and_private() {
        let root =
            std::env::temp_dir().join(format!("monocode-soul-recovery-{}", uuid::Uuid::new_v4()));
        let first = save_soul_recovery(&root, "mono", "private first").unwrap();
        let second = save_soul_recovery(&root, "mono", "private second").unwrap();
        assert_ne!(first, second);
        assert_eq!(std::fs::read_to_string(&first).unwrap(), "private first");
        assert_eq!(std::fs::read_to_string(&second).unwrap(), "private second");
        let larger = "x".repeat(FILE_MAX + 1);
        let third = save_soul_recovery(&root, "mono", &larger).unwrap();
        assert_eq!(std::fs::read_to_string(third).unwrap(), larger);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&first).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        std::fs::remove_dir_all(root).unwrap();
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
}
