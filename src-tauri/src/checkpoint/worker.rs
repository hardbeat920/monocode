use super::*;
use sha2::{Digest, Sha256};
use std::io::Read;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct Baseline {
    pub head: Vec<u8>,
    pub skipped: BTreeMap<String, String>,
}

pub(super) fn checkout_paths(root: &Path) -> Result<BTreeSet<String>, String> {
    let mut paths = git_paths(root, &["ls-tree", "-r", "--name-only", "-z", "HEAD"])?;
    paths.extend(git_paths(
        root,
        &[
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ],
    )?);
    Ok(paths)
}

fn git_paths(root: &Path, args: &[&str]) -> Result<BTreeSet<String>, String> {
    let mut command = Command::new("git");
    crate::hide_window_console(&mut command);
    let output = command
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    String::from_utf8(output.stdout)
        .map_err(|_| "The worker has a non-UTF-8 path".to_string())?
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(|path| {
            let resolved = resolve_repo_path(root, path)?;
            if resolved != path {
                return Err(format!("Cannot checkpoint ambiguous path {path:?}"));
            }
            Ok(resolved)
        })
        .collect()
}

pub(super) fn fingerprint(root: &Path, relative: &str) -> Result<String, String> {
    let path = root.join(relative);
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok("missing".into()),
        Err(error) => return Err(error.to_string()),
    };
    if metadata.file_type().is_symlink() {
        return Ok(format!(
            "link:{:?}",
            std::fs::read_link(path).map_err(|e| e.to_string())?
        ));
    }
    if metadata.is_dir() {
        let mut entries = std::fs::read_dir(&path)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        entries.sort_by_key(|entry| entry.file_name());
        let mut hash = Sha256::new();
        for entry in entries {
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| "Cannot checkpoint a non-UTF-8 directory entry".to_string())?;
            hash.update((name.len() as u64).to_le_bytes());
            hash.update(name.as_bytes());
            hash.update(fingerprint(root, &format!("{relative}/{name}"))?.as_bytes());
        }
        return Ok(format!("directory:{:x}", hash.finalize()));
    }
    if !metadata.is_file() {
        return Err(format!(
            "Cannot checkpoint non-file {relative}. The worker worktree was kept."
        ));
    }
    let mut file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}:{:?}", hash.finalize(), file_mode(&path)))
}

impl CheckpointStore {
    pub(super) fn ensure_worker(&self, session_id: &str, cwd: &str) -> Result<(), String> {
        let root = project_root(cwd)?;
        let dir = self.session_dir(session_id);
        if let Some(manifest) = read_manifest(&dir)? {
            if same_cwd(&manifest.cwd, cwd) && manifest.worker.is_some() {
                return Ok(());
            }
            return Err("This retained worker has no original isolated baseline. Its checkout was kept for recovery.".into());
        }
        let mut baseline = Baseline {
            head: git_head(&root)?,
            skipped: BTreeMap::new(),
        };
        let mut files = BTreeMap::new();
        for relative in checkout_paths(&root)? {
            let snapshot = snapshot_file(&dir, &root, &relative)?;
            if snapshot == SnapshotKind::Skipped {
                baseline
                    .skipped
                    .insert(relative.clone(), fingerprint(&root, &relative)?);
            }
            files.insert(relative, snapshot);
        }
        write_manifest(
            &dir,
            &Manifest {
                cwd: root.to_string_lossy().into_owned(),
                worker: Some(baseline),
                after_generation: None,
                files,
                touched: BTreeSet::new(),
                tracked: BTreeSet::new(),
                prepared: BTreeSet::new(),
                after: BTreeMap::new(),
                stats: BTreeMap::new(),
                diverged: BTreeSet::new(),
            },
        )
    }

    pub(super) fn capture_worker(
        &self,
        session_id: &str,
        cwd: &str,
        scopes: &[String],
    ) -> Result<(), String> {
        let root = project_root(cwd)?;
        let dir = self.session_dir(session_id);
        let mut manifest = self.load_matching(session_id, cwd)?.ok_or(
            "The worker has no original isolated baseline. Its checkout was kept for recovery.",
        )?;
        let baseline = manifest.worker.as_ref().ok_or(
            "The worker has no original isolated baseline. Its checkout was kept for recovery.",
        )?;
        if baseline.head != git_head(&root)? {
            return Err(
                "The worker branch moved after its baseline. The worktree was kept.".into(),
            );
        }
        let scopes = normalized_scopes(scopes)?;
        let mut paths = checkout_paths(&root)?;
        paths.extend(manifest.files.keys().cloned());
        let mut changed = Vec::new();
        for relative in paths {
            let before = manifest
                .files
                .get(&relative)
                .copied()
                .unwrap_or(SnapshotKind::Missing);
            if before == SnapshotKind::Skipped {
                if baseline.skipped.get(&relative) == Some(&fingerprint(&root, &relative)?) {
                    continue;
                }
                return Err(format!(
                    "Unsupported file {relative} changed. The worker worktree was kept."
                ));
            }
            if path_contains_symlink(&root, &relative) {
                return Err(format!(
                    "Cannot capture symbolic link {relative}. The worker worktree was kept."
                ));
            }
            if before == SnapshotKind::Contents
                && read_snapshot(&dir, &relative, before) == FileState::Missing
            {
                return Err(format!(
                    "Missing baseline snapshot for {relative}. The worker worktree was kept."
                ));
            }
            if stored_snapshot(&dir, &manifest, &relative, before, false)
                != worktree_snapshot(&root, &relative)
            {
                let key = scope_key(&relative);
                if !scopes.iter().any(|scope| {
                    scope == "." || key == *scope || key.starts_with(&format!("{scope}/"))
                }) {
                    return Err(format!(
                        "Worker changed {relative} outside its assignment. The worktree was kept."
                    ));
                }
                changed.push((relative, before));
            }
        }
        if changed.len() > MAX_SNAPSHOT_FILES {
            return Err(
                "Too many changed files to checkpoint. The worker worktree was kept.".into(),
            );
        }
        manifest.touched.clear();
        manifest.tracked.clear();
        manifest.prepared.clear();
        manifest.after.clear();
        manifest.stats.clear();
        std::fs::create_dir_all(dir.join("captures")).map_err(|e| e.to_string())?;
        let capture_root = loop {
            manifest.after_generation = Some(
                manifest
                    .after_generation
                    .unwrap_or(0)
                    .checked_add(1)
                    .ok_or("Worker checkpoint generation overflow")?,
            );
            let path = after_root(&dir, &manifest);
            match std::fs::create_dir(&path) {
                Ok(()) => break path,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.to_string()),
            }
        };
        for (relative, before) in changed {
            manifest.files.entry(relative.clone()).or_insert(before);
            let after = snapshot_file_at(&capture_root, &root, &relative)?;
            if after == SnapshotKind::Skipped {
                return Err(format!(
                    "Cannot capture unsupported file {relative}. The worker worktree was kept."
                ));
            }
            manifest.after.insert(relative.clone(), after);
            manifest.prepared.insert(relative.clone());
            manifest.touched.insert(relative.clone());
            if in_head(&root, &relative) {
                manifest.tracked.insert(relative.clone());
            }
            if let Some(stats) = calculate_session_stats(&dir, &manifest, &relative) {
                manifest.stats.insert(relative, stats);
            }
        }
        write_manifest(&dir, &manifest)
    }
}

fn scope_key(path: &str) -> String {
    if cfg!(windows) {
        path.to_lowercase()
    } else {
        path.to_string()
    }
}

fn normalized_scopes(scopes: &[String]) -> Result<Vec<String>, String> {
    if scopes.is_empty() {
        return Err("Worker scope is empty".into());
    }
    scopes
        .iter()
        .map(|scope| {
            let scope = scope.replace('\\', "/");
            let scope = scope.trim_end_matches('/');
            if scope != "."
                && (scope.contains(':')
                    || scope
                        .split('/')
                        .any(|part| part.is_empty() || part == "." || part == ".."))
            {
                return Err("Invalid worker scope".into());
            }
            Ok(scope_key(scope))
        })
        .collect()
}

#[tauri::command]
pub async fn worker_checkpoint_ensure(
    store: State<'_, CheckpointStore>,
    session_id: String,
    cwd: String,
) -> Result<(), String> {
    validate_id(&session_id, "session")?;
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.exclusive(|store| store.ensure_worker(&session_id, &cwd))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn worker_checkpoint_capture(
    store: State<'_, CheckpointStore>,
    session_id: String,
    cwd: String,
    scopes: Vec<String>,
) -> Result<(), String> {
    validate_id(&session_id, "session")?;
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.exclusive(|store| store.capture_worker(&session_id, &cwd, &scopes))
    })
    .await
    .map_err(|e| e.to_string())?
}
