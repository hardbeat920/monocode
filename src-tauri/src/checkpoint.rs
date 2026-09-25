use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

#[cfg(test)]
use crate::fs::GitDiffStats;
use crate::fs::{
    expand_home, git_checked, git_diff_files_for, path_to_js, resolve_repo_path, GitChangedFile,
    GitDiffIndex, MAX_TEXT_FILE_BYTES,
};

const MAX_SNAPSHOT_FILES: usize = 500;

#[derive(Clone)]
pub struct CheckpointStore {
    root: PathBuf,
    gate: Arc<Mutex<()>>,
}

impl CheckpointStore {
    fn new(root: PathBuf) -> Self {
        Self {
            root,
            gate: Arc::new(Mutex::new(())),
        }
    }

    fn exclusive<T>(
        &self,
        operation: impl FnOnce(&Self) -> Result<T, String>,
    ) -> Result<T, String> {
        let _guard = self
            .gate
            .lock()
            .map_err(|_| "Checkpoint store lock poisoned".to_string())?;
        operation(self)
    }

    fn session_dir(&self, session_id: &str) -> PathBuf {
        self.root.join(session_id)
    }

    fn ensure(&self, session_id: &str, cwd: &str, isolated: bool) -> Result<(), String> {
        let root = project_root(cwd)?;
        let dir = self.session_dir(session_id);
        if let Some(manifest) = read_manifest(&dir)? {
            if same_cwd(&manifest.cwd, cwd) {
                return Ok(());
            }
            let _ = std::fs::remove_dir_all(&dir);
        }
        if isolated {
            return ensure_isolated(&dir, &root);
        }
        std::fs::create_dir_all(dir.join("files")).map_err(|e| e.to_string())?;

        let mut files = BTreeMap::new();
        let mut tracked = BTreeSet::new();
        for file in git_diff_files_for(&root).files {
            if files.len() >= MAX_SNAPSHOT_FILES {
                break;
            }
            let Ok(relative) = resolve_repo_path(&root, &file.relative) else {
                continue;
            };
            if in_head(&root, &relative) {
                tracked.insert(relative.clone());
            }
            files.insert(relative.clone(), snapshot_file(&dir, &root, &relative)?);
        }
        write_manifest(
            &dir,
            &Manifest {
                cwd: root.to_string_lossy().into_owned(),
                files,
                touched: BTreeSet::new(),
                tracked,
                prepared: BTreeSet::new(),
                after: BTreeMap::new(),
                stats: BTreeMap::new(),
                diverged: BTreeSet::new(),
                isolated: false,
                seed: BTreeMap::new(),
                seed_stat: BTreeMap::new(),
                seed_ignored: BTreeSet::new(),
            },
        )
    }

    fn prepare(&self, session_id: &str, cwd: &str, paths: &[String]) -> Result<(), String> {
        if paths.is_empty() {
            return Ok(());
        }
        let root = project_root(cwd)?;
        let dir = self.session_dir(session_id);
        let mut manifest = match read_manifest(&dir)? {
            Some(manifest) if same_cwd(&manifest.cwd, cwd) => manifest,
            _ => return Ok(()),
        };

        let mut dirty = false;
        for path in paths {
            let Ok(relative) = relative_to_root(&root, path) else {
                continue;
            };
            // Keep the original pre-edit snapshot across later edits by this
            // session. The first tool-start event owns the safe undo boundary.
            if manifest.touched.contains(&relative) && manifest.prepared.contains(&relative) {
                if !after_matches_worktree(&dir, &root, &manifest, &relative)
                    && manifest.diverged.insert(relative)
                {
                    dirty = true;
                }
                continue;
            }
            if manifest.prepared.contains(&relative) {
                continue;
            }
            if manifest.touched.contains(&relative) {
                // Upgrade a legacy or completion-only claim by dropping its
                // untrusted state and starting at this real tool boundary.
                release_path(&mut manifest, &relative);
                dirty = true;
            }
            let before = snapshot_file(&dir, &root, &relative)?;
            if manifest.files.insert(relative.clone(), before) != Some(before) {
                dirty = true;
            }
            if manifest.prepared.insert(relative.clone()) {
                dirty = true;
            }
            if in_head(&root, &relative) && manifest.tracked.insert(relative) {
                dirty = true;
            }
        }
        if dirty {
            write_manifest(&dir, &manifest)?;
        }
        Ok(())
    }

    fn capture(&self, session_id: &str, cwd: &str, paths: &[String]) -> Result<(), String> {
        if paths.is_empty() {
            return Ok(());
        }
        let root = project_root(cwd)?;
        let dir = self.session_dir(session_id);
        let mut manifest = match read_manifest(&dir)? {
            Some(manifest) if same_cwd(&manifest.cwd, cwd) => manifest,
            _ => return Ok(()),
        };

        let mut dirty = false;
        for path in paths {
            if manifest.touched.len() >= MAX_SNAPSHOT_FILES {
                break;
            }
            let Ok(relative) = relative_to_root(&root, path) else {
                continue;
            };
            manifest.touched.insert(relative.clone());
            let tracked_in_head = in_head(&root, &relative);
            if tracked_in_head {
                manifest.tracked.insert(relative.clone());
            }
            if !manifest.files.contains_key(&relative)
                && !root.join(&relative).exists()
                && !tracked_in_head
            {
                // A completion without a matching prepare event is retained
                // for review but is deliberately not undoable.
                manifest
                    .files
                    .insert(relative.clone(), snapshot_file(&dir, &root, &relative)?);
            }
            let after = snapshot_after_file(&dir, &root, &relative)?;
            manifest.after.insert(relative.clone(), after);
            if let Some(stats) = calculate_session_stats(&dir, &manifest, &relative) {
                manifest.stats.insert(relative, stats);
            }
            dirty = true;
        }
        if dirty {
            write_manifest(&dir, &manifest)?;
        }
        Ok(())
    }

    fn status(&self, session_id: &str, cwd: &str) -> Result<CheckpointStatus, String> {
        let Some(manifest) = self.load_matching(session_id, cwd)? else {
            return Ok(CheckpointStatus { files: Vec::new() });
        };
        let root = project_root(cwd)?;
        let foreign_touched = self.foreign_touched_paths(cwd, session_id);
        Ok(diff_from_manifest(
            &self.session_dir(session_id),
            &root,
            &manifest,
            &foreign_touched,
        ))
    }

    fn apply(
        &self,
        session_id: &str,
        from_cwd: &str,
        to_cwd: &str,
        write_scopes: Option<&[String]>,
    ) -> Result<CheckpointApplyResult, String> {
        let manifest = self.load_matching(session_id, from_cwd)?.ok_or_else(|| {
            format!(
                "This worker has no change checkpoint, so MonoCode cannot tell which changes are its own. Its edits are still in {from_cwd}. Copy them into the lead checkout by hand, then cancel the task."
            )
        })?;
        let from_root = project_root(from_cwd)?;
        let to_root = project_root(to_cwd)?;
        if same_cwd(from_cwd, to_cwd) {
            return Err("An isolated worker cannot be integrated into itself".into());
        }
        if git_head(&from_root)? != git_head(&to_root)? {
            return Err(
                "The worker or lead branch moved while this task was running. The worker worktree was kept for manual review."
                    .into(),
            );
        }
        if manifest.isolated {
            // Git reports paths from the top level, so a subfolder checkout
            // would write them to the wrong place.
            for (root, label) in [(&from_root, "worker"), (&to_root, "lead")] {
                if !git_bytes(root, &["rev-parse", "--show-prefix"])?
                    .trim_ascii()
                    .is_empty()
                {
                    return Err(format!(
                        "Cannot integrate this worker: the {label} checkout {} is a subfolder, not the top level of its repository. The worker worktree was kept.",
                        root.display()
                    ));
                }
            }
        }
        let dir = self.session_dir(session_id);
        let delta = if manifest.isolated {
            isolated_worker_delta(&dir, &from_root, &manifest)?
        } else {
            let changes = verified_worker_delta(&dir, &from_root, &manifest)?
                .into_iter()
                .map(|relative| {
                    let before = manifest
                        .files
                        .get(&relative)
                        .copied()
                        .ok_or_else(|| format!("Missing original snapshot for {relative}"))?;
                    let after = manifest
                        .after
                        .get(&relative)
                        .copied()
                        .ok_or_else(|| format!("Missing worker snapshot for {relative}"))?;
                    Ok(WorkerChange {
                        before: stored_snapshot(&dir, &relative, before, false),
                        after: stored_snapshot(&dir, &relative, after, true),
                        head_before: false,
                        relative,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            WorkerDelta {
                changes,
                renames: Vec::new(),
                ignored: Vec::new(),
                unconfirmed: Vec::new(),
            }
        };
        // Files outside the assignment stay in the worker worktree for the
        // lead to review. The caller must keep that worktree while any remain.
        let allowed = write_scopes.map(|scopes| scope_parts(&from_root, scopes));
        let in_scope = |relative: &str| {
            allowed
                .as_ref()
                .map_or(true, |allowed| in_scopes(allowed, relative))
        };
        let mut skipped = Vec::new();
        let mut renames = Vec::new();
        for rename in &delta.renames {
            if in_scope(&rename.to) {
                renames.push(rename.clone());
            } else {
                skipped.push(rename.label());
            }
        }
        let mut changes = Vec::new();
        for change in &delta.changes {
            // A file under a renamed folder is judged by its new spelling.
            if in_scope(&renamed_path(&change.relative, &delta.renames)) {
                changes.push(change.clone());
            } else {
                skipped.push(change.relative.clone());
            }
        }
        skipped.sort();
        // Git records only the executable bit, so an isolated delta whose
        // before state came from HEAD cannot know the lead's exact mode.
        let loose_mode = manifest.isolated;

        // Preflight every path before writing any of them. A retry may see a
        // mixture of before/after states if the app stopped during a previous
        // application; both are safe and make this operation idempotent.
        // Finish a case rename an earlier apply left under its temporary
        // name, so the preflight sees the lead as that apply left it.
        for rename in &renames {
            let (parent, _) = split_last(&rename.from);
            let (_, new) = split_last(&rename.to);
            if lead_rename(&to_root, parent, rename, &changes)? == RenameState::Resume {
                let parent = to_root.join(parent);
                std::fs::rename(case_temp(&parent, new), parent.join(new))
                    .map_err(|e| e.to_string())?;
            }
        }
        let already_applied = preflight(&to_root, &changes, &renames, loose_mode)?;
        let aliases = unconfirmed_aliases(&to_root, &delta.unconfirmed, &changes)?;
        let worker = manifest.isolated.then_some(from_root.as_path());
        write_changes(&to_root, &changes, &renames, &aliases, loose_mode, worker)?;
        if manifest.isolated {
            verify_worker_settled(&dir, &from_root, &manifest, &delta)?;
        }

        let ignored = delta
            .ignored
            .into_iter()
            .filter(|entry| !lead_has_same_entry(&from_root, &to_root, entry))
            .collect();
        let mut files: Vec<String> = changes
            .into_iter()
            .map(|change| change.relative)
            .chain(renames.iter().map(CheckpointRename::label))
            .collect();
        files.sort();
        Ok(CheckpointApplyResult {
            files,
            already_applied,
            skipped,
            renamed: renames,
            ignored,
        })
    }

    fn cleanup_safe(&self, session_id: &str, cwd: &str) -> Result<bool, String> {
        let Some(manifest) = self.load_matching(session_id, cwd)? else {
            return Ok(false);
        };
        let root = project_root(cwd)?;
        let dir = self.session_dir(session_id);
        let clean = if manifest.isolated {
            // Worker-created ignored files count too. Cleanup has no lead
            // to compare them with, so any of them keeps the worktree.
            isolated_worker_delta(&dir, &root, &manifest).map(|delta| {
                delta.changes.is_empty() && delta.renames.is_empty() && delta.ignored.is_empty()
            })
        } else {
            verified_worker_delta(&dir, &root, &manifest).map(|changed| changed.is_empty())
        };
        Ok(clean.unwrap_or(false))
    }

    fn forget(&self, session_id: &str) -> Result<(), String> {
        let dir = self.session_dir(session_id);
        if dir.exists() {
            std::fs::remove_dir_all(dir).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn file_diff(
        &self,
        session_id: &str,
        cwd: &str,
        relative: &str,
    ) -> Result<CheckpointFileDiff, String> {
        let Some(manifest) = self.load_matching(session_id, cwd)? else {
            return Err("Session changes are no longer available".into());
        };
        let root = project_root(cwd)?;
        let relative = resolve_repo_path(&root, relative)?;
        if !manifest.touched.contains(&relative) || !manifest.prepared.contains(&relative) {
            return Err("This file was not changed by the session".into());
        }
        if manifest.diverged.contains(&relative) {
            return Err(
                "Exact lines are unavailable because the file changed between this session's edits"
                    .into(),
            );
        }

        let dir = self.session_dir(session_id);
        let before = manifest
            .files
            .get(&relative)
            .copied()
            .ok_or_else(|| "Session baseline is unavailable".to_string())?;
        let after = manifest
            .after
            .get(&relative)
            .copied()
            .ok_or_else(|| "Session result is unavailable".to_string())?;
        let original = read_snapshot(&dir, &relative, before);
        let current = read_after_snapshot(&dir, &relative, after);
        let too_large =
            matches!(original, FileState::Skipped) || matches!(current, FileState::Skipped);
        let binary = state_is_binary(&original) || state_is_binary(&current);
        let (original, current) = if binary || too_large {
            (String::new(), String::new())
        } else {
            (state_text(original), state_text(current))
        };
        let status = manifest
            .stats
            .get(&relative)
            .map(|stats| stats.status.clone())
            .unwrap_or_else(|| "modified".into());
        Ok(CheckpointFileDiff {
            path: path_to_js(&root.join(&relative)),
            relative,
            status,
            original,
            current,
            binary,
            too_large,
        })
    }

    /// Remaining git line counts for each session, using one working-tree index.
    #[cfg(test)]
    fn stats_for_sessions(
        &self,
        cwd: &str,
        session_ids: &[String],
    ) -> Result<HashMap<String, GitDiffStats>, String> {
        let mut out = HashMap::new();
        if session_ids.is_empty() {
            return Ok(out);
        }
        let root = project_root(cwd)?;
        let index = git_diff_files_for(&root);
        for session_id in session_ids {
            let Some(manifest) = self.load_matching(session_id, cwd)? else {
                out.insert(session_id.clone(), GitDiffStats::default());
                continue;
            };
            let foreign_touched = self.foreign_touched_paths(cwd, session_id);
            let status = diff_from_manifest_with(
                &index,
                &self.session_dir(session_id),
                &root,
                &manifest,
                &foreign_touched,
            );
            out.insert(session_id.clone(), stats_from_status(&status));
        }
        Ok(out)
    }

    fn undo(
        &self,
        session_id: &str,
        cwd: &str,
        relative: Option<&str>,
    ) -> Result<CheckpointStatus, String> {
        let Some(mut manifest) = self.load_matching(session_id, cwd)? else {
            return Ok(CheckpointStatus { files: Vec::new() });
        };
        let root = project_root(cwd)?;
        let dir = self.session_dir(session_id);
        let foreign_touched = self.foreign_touched_paths(cwd, session_id);
        let changed = diff_from_manifest(&dir, &root, &manifest, &foreign_touched);
        if let Some(relative) = relative {
            let relative = resolve_repo_path(&root, relative)?;
            let Some(file) = changed.files.iter().find(|file| file.relative == relative) else {
                return self.status(session_id, cwd);
            };
            if !file.undoable {
                return Err(format!(
                    "Cannot safely undo {relative}: it changed outside this session"
                ));
            }
            restore_one(&dir, &root, &manifest, &relative)?;
            release_path(&mut manifest, &relative);
            write_manifest(&dir, &manifest)?;
            return self.status(session_id, cwd);
        }
        if changed.files.iter().any(|file| !file.undoable) {
            return Err(
                "Cannot safely undo all: one or more files changed outside this session".into(),
            );
        }
        for file in &changed.files {
            restore_one(&dir, &root, &manifest, &file.relative)?;
        }
        let _ = std::fs::remove_dir_all(&dir);
        Ok(CheckpointStatus { files: Vec::new() })
    }

    fn keep(
        &self,
        session_id: &str,
        cwd: &str,
        relative: Option<&str>,
    ) -> Result<CheckpointStatus, String> {
        let Some(mut manifest) = self.load_matching(session_id, cwd)? else {
            return Ok(CheckpointStatus { files: Vec::new() });
        };
        let root = project_root(cwd)?;
        let dir = self.session_dir(session_id);
        let Some(relative) = relative else {
            if manifest.isolated {
                // Integration reads the delta from the seed and the live
                // worktree, so keep those and clear only the review state.
                clear_review(&dir, &mut manifest);
                write_manifest(&dir, &manifest)?;
            } else {
                let _ = std::fs::remove_dir_all(&dir);
            }
            return Ok(CheckpointStatus { files: Vec::new() });
        };
        let relative = resolve_repo_path(&root, relative)?;
        release_path(&mut manifest, &relative);
        write_manifest(&dir, &manifest)?;
        self.status(session_id, cwd)
    }

    fn load_matching(&self, session_id: &str, cwd: &str) -> Result<Option<Manifest>, String> {
        let dir = self.session_dir(session_id);
        let Some(manifest) = read_manifest(&dir)? else {
            return Ok(None);
        };
        if !same_cwd(&manifest.cwd, cwd) {
            return Ok(None);
        }
        Ok(Some(manifest))
    }

    /// Paths already claimed by another live session in the same project.
    fn foreign_touched_paths(&self, cwd: &str, except_session_id: &str) -> HashSet<String> {
        let mut paths = HashSet::new();
        let entries = match std::fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(_) => return paths,
        };
        for entry in entries.flatten() {
            let session_id = entry.file_name().to_string_lossy().into_owned();
            if session_id == except_session_id {
                continue;
            }
            let dir = entry.path();
            let Ok(Some(manifest)) = read_manifest(&dir) else {
                continue;
            };
            if !same_cwd(&manifest.cwd, cwd) {
                continue;
            }
            paths.extend(manifest.touched.intersection(&manifest.prepared).cloned());
        }
        paths
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    cwd: String,
    files: BTreeMap<String, SnapshotKind>,
    #[serde(default)]
    touched: BTreeSet<String>,
    #[serde(default)]
    tracked: BTreeSet<String>,
    /// Paths captured before a structured edit started. Only these are safe
    /// candidates for Undo.
    #[serde(default)]
    prepared: BTreeSet<String>,
    /// Worktree contents immediately after the session's latest edit.
    #[serde(default)]
    after: BTreeMap<String, SnapshotKind>,
    /// Stable line counts for the session-owned before/after pair.
    #[serde(default)]
    stats: BTreeMap<String, ChangeStats>,
    /// Paths whose contents changed between two edits by this session.
    #[serde(default)]
    diverged: BTreeSet<String>,
    /// The session owns its whole worktree, so its delta is read from the
    /// live files against `seed` and HEAD instead of from tool events.
    #[serde(default)]
    isolated: bool,
    /// Dirty files when the isolated worktree was created. Kept apart from
    /// `files` because a later prepare replaces those snapshots.
    #[serde(default)]
    seed: BTreeMap<String, SnapshotKind>,
    /// Seed files too large to snapshot. The delta treats one as unchanged
    /// while the live worker file still has this size, mtime, and mode.
    #[serde(default)]
    seed_stat: BTreeMap<String, FileStat>,
    /// Ignored files and folders present when the isolated worktree was
    /// created, as `ignored_listing` reports them. Later ones are the
    /// worker's own.
    #[serde(default)]
    seed_ignored: BTreeSet<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct FileStat {
    size: u64,
    mtime_ns: i64,
    mode: Option<u32>,
}

/// Size, mtime, and mode of a regular file. None for anything else,
/// including symlinks and unreadable metadata.
fn file_stat(path: &Path) -> Option<FileStat> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let mtime_ns = match meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_nanos()).ok()?,
        Err(before) => -i64::try_from(before.duration().as_nanos()).ok()?,
    };
    Some(FileStat {
        size: meta.len(),
        mtime_ns,
        mode: file_mode(path),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct ChangeStats {
    status: String,
    additions: i64,
    deletions: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum SnapshotKind {
    Contents,
    Missing,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FileState {
    Contents(Vec<u8>),
    Missing,
    Skipped,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointFile {
    pub path: String,
    pub relative: String,
    pub status: String,
    pub additions: i64,
    pub deletions: i64,
    /// False when the file changed between this session's own edit snapshots,
    /// so its net line ownership cannot be reconstructed exactly.
    pub exact: bool,
    pub undoable: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointStatus {
    pub files: Vec<CheckpointFile>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointFileDiff {
    pub path: String,
    pub relative: String,
    pub status: String,
    pub original: String,
    pub current: String,
    pub binary: bool,
    pub too_large: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointApplyResult {
    pub files: Vec<String>,
    pub already_applied: usize,
    /// Changed files outside the write scopes, left in the worker worktree.
    pub skipped: Vec<String>,
    /// Case-only renames written to the lead. `files` and `skipped` also
    /// list each one as "Foo.ts -> foo.ts".
    pub renamed: Vec<CheckpointRename>,
    /// Ignored files and folders the worker created that the lead lacks or
    /// has with other contents. They are never written. A folder ends in `/`.
    pub ignored: Vec<String>,
}

/// A path whose name changed only in letter case. `from` is the name git
/// records and `to` the worker's spelling on disk.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointRename {
    pub from: String,
    pub to: String,
}

impl CheckpointRename {
    fn label(&self) -> String {
        format!("{} -> {}", self.from, self.to)
    }
}

pub fn init(app: &AppHandle) -> Result<(), String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("checkpoints");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    app.manage(CheckpointStore::new(dir));
    Ok(())
}

#[tauri::command]
pub async fn session_checkpoint_ensure(
    store: State<'_, CheckpointStore>,
    session_id: String,
    cwd: String,
    isolated: Option<bool>,
) -> Result<(), String> {
    validate_id(&session_id, "session")?;
    let isolated = isolated.unwrap_or(false);
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.exclusive(|store| store.ensure(&session_id, &cwd, isolated))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn session_checkpoint_prepare(
    store: State<'_, CheckpointStore>,
    session_id: String,
    cwd: String,
    paths: Vec<String>,
) -> Result<(), String> {
    validate_id(&session_id, "session")?;
    if paths.len() > MAX_SNAPSHOT_FILES {
        return Err("Too many paths".into());
    }
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.exclusive(|store| store.prepare(&session_id, &cwd, &paths))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn session_checkpoint_capture(
    store: State<'_, CheckpointStore>,
    session_id: String,
    cwd: String,
    paths: Vec<String>,
) -> Result<(), String> {
    validate_id(&session_id, "session")?;
    if paths.len() > MAX_SNAPSHOT_FILES {
        return Err("Too many paths".into());
    }
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.exclusive(|store| store.capture(&session_id, &cwd, &paths))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn session_checkpoint_status(
    store: State<'_, CheckpointStore>,
    session_id: String,
    cwd: String,
) -> Result<CheckpointStatus, String> {
    validate_id(&session_id, "session")?;
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.exclusive(|store| store.status(&session_id, &cwd))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn session_checkpoint_apply(
    store: State<'_, CheckpointStore>,
    session_id: String,
    from_cwd: String,
    to_cwd: String,
    write_scopes: Option<Vec<String>>,
) -> Result<CheckpointApplyResult, String> {
    validate_id(&session_id, "session")?;
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.exclusive(|store| {
            store.apply(&session_id, &from_cwd, &to_cwd, write_scopes.as_deref())
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn session_checkpoint_cleanup_safe(
    store: State<'_, CheckpointStore>,
    session_id: String,
    cwd: String,
) -> Result<bool, String> {
    validate_id(&session_id, "session")?;
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.exclusive(|store| store.cleanup_safe(&session_id, &cwd))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn session_checkpoint_forget(
    store: State<'_, CheckpointStore>,
    session_id: String,
) -> Result<(), String> {
    validate_id(&session_id, "session")?;
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || store.exclusive(|store| store.forget(&session_id)))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn session_checkpoint_file_diff(
    store: State<'_, CheckpointStore>,
    session_id: String,
    cwd: String,
    relative: String,
) -> Result<CheckpointFileDiff, String> {
    validate_id(&session_id, "session")?;
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.exclusive(|store| store.file_diff(&session_id, &cwd, &relative))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn session_checkpoint_undo(
    store: State<'_, CheckpointStore>,
    session_id: String,
    cwd: String,
    relative: Option<String>,
) -> Result<CheckpointStatus, String> {
    validate_id(&session_id, "session")?;
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.exclusive(|store| store.undo(&session_id, &cwd, relative.as_deref()))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn session_checkpoint_keep(
    store: State<'_, CheckpointStore>,
    session_id: String,
    cwd: String,
    relative: Option<String>,
) -> Result<CheckpointStatus, String> {
    validate_id(&session_id, "session")?;
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.exclusive(|store| store.keep(&session_id, &cwd, relative.as_deref()))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Reconstruct the worker-owned delta and reject anything that was not
/// captured at a structured tool boundary. This is stricter than the review
/// UI because cleanup must never discard an ambiguous edit.
fn verified_worker_delta(
    dir: &Path,
    root: &Path,
    manifest: &Manifest,
) -> Result<Vec<String>, String> {
    if !manifest.diverged.is_empty() {
        return Err(format!(
            "Cannot safely integrate files that changed outside the worker: {}",
            manifest
                .diverged
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    let current_dirty: BTreeSet<String> = git_diff_files_for(root)
        .files
        .into_iter()
        .map(|file| file.relative)
        .collect();
    if let Some(relative) = current_dirty
        .iter()
        .find(|relative| !manifest.files.contains_key(*relative))
    {
        return Err(format!(
            "Cannot safely integrate {relative}: its change was not captured for this worker. The worker worktree was kept."
        ));
    }

    let mut changed = Vec::new();
    for (relative, before) in &manifest.files {
        if path_contains_symlink(root, relative) {
            return Err(format!(
                "Cannot safely integrate {relative}: the worker path contains a symbolic link. The worker worktree was kept."
            ));
        }
        if *before == SnapshotKind::Skipped {
            return Err(format!(
                "Cannot safely integrate {relative}: this file type or size cannot be checkpointed. The worker worktree was kept."
            ));
        }
        let before_state = stored_snapshot(dir, relative, *before, false);
        if manifest.touched.contains(relative) {
            if !manifest.prepared.contains(relative) {
                return Err(format!(
                    "Cannot safely integrate {relative}: its pre-edit state was not captured. The worker worktree was kept."
                ));
            }
            let after = manifest
                .after
                .get(relative)
                .copied()
                .ok_or_else(|| format!("Missing worker snapshot for {relative}"))?;
            if after == SnapshotKind::Skipped {
                return Err(format!(
                    "Cannot safely integrate {relative}: this file type or size cannot be checkpointed. The worker worktree was kept."
                ));
            }
            let after_state = stored_snapshot(dir, relative, after, true);
            if worktree_snapshot(root, relative) != after_state {
                return Err(format!(
                    "Cannot safely integrate {relative}: it changed after the worker checkpoint. The worker worktree was kept."
                ));
            }
            if before_state != after_state {
                changed.push(relative.clone());
            }
        } else if worktree_snapshot(root, relative) != before_state {
            return Err(format!(
                "Cannot safely integrate {relative}: its change was not attributed to this worker. The worker worktree was kept."
            ));
        }
    }
    changed.sort();
    Ok(changed)
}

/// Record an isolated worker's seed baseline. Every dirty path must fit,
/// because the delta treats any path missing from the seed as HEAD.
fn ensure_isolated(dir: &Path, root: &Path) -> Result<(), String> {
    let dirty = worktree_dirty_paths(root)?;
    let paths: BTreeSet<&String> = dirty.changed.iter().chain(&dirty.untracked).collect();
    if paths.len() > MAX_SNAPSHOT_FILES {
        return Err(format!(
            "This checkout has too many uncommitted files to isolate a worker ({}, the limit is {MAX_SNAPSHOT_FILES}). Commit or stash some of them, then try again.",
            paths.len()
        ));
    }
    std::fs::create_dir_all(dir.join("files")).map_err(|e| e.to_string())?;

    let mut files = BTreeMap::new();
    let mut seed = BTreeMap::new();
    let mut seed_stat = BTreeMap::new();
    let mut tracked = BTreeSet::new();
    for relative in paths {
        if in_head(root, relative) {
            tracked.insert(relative.clone());
        }
        files.insert(relative.clone(), snapshot_file(dir, root, relative)?);
        let kind = snapshot_file_at(&dir.join("seed"), root, relative)?;
        if kind == SnapshotKind::Skipped {
            // Too large to copy. Remember its stat so an untouched large
            // file does not block every later integration.
            if let Some(stat) =
                file_stat(&root.join(relative)).filter(|stat| stat.size > MAX_TEXT_FILE_BYTES)
            {
                seed_stat.insert(relative.clone(), stat);
            }
        }
        seed.insert(relative.clone(), kind);
    }
    write_manifest(
        dir,
        &Manifest {
            cwd: root.to_string_lossy().into_owned(),
            files,
            touched: BTreeSet::new(),
            tracked,
            prepared: BTreeSet::new(),
            after: BTreeMap::new(),
            stats: BTreeMap::new(),
            diverged: BTreeSet::new(),
            isolated: true,
            seed,
            seed_stat,
            seed_ignored: ignored_listing(root)?,
        },
    )
}

/// One file the worker changed, with the state it started from and the
/// state it ended in.
#[derive(Clone)]
struct WorkerChange {
    relative: String,
    before: (FileState, Option<u32>),
    after: (FileState, Option<u32>),
    /// `before` came from HEAD through the worker's own filters, so line
    /// endings may differ from the lead's checkout of the same blob.
    head_before: bool,
}

/// Everything an isolated worker changed in its worktree.
struct WorkerDelta {
    changes: Vec<WorkerChange>,
    /// Case-only renames, shallowest first. Content changes stay keyed by
    /// the git name.
    renames: Vec<CheckpointRename>,
    /// Ignored files and folders the worker created. A folder ends in `/`.
    ignored: Vec<String>,
    /// Folder case renames seen only on paths the worker deleted, so no
    /// surviving file confirms them. `apply` writes under the new spelling
    /// only when the lead's old folder is gone by then.
    unconfirmed: Vec<CheckpointRename>,
}

struct DirtyPaths {
    /// Paths that differ from HEAD, including staged and deleted files.
    changed: BTreeSet<String>,
    /// Untracked, non-ignored files. Git lists each file inside a new
    /// directory rather than the directory itself.
    untracked: BTreeSet<String>,
}

/// Every path that differs from HEAD. Unlike `git_diff_files_for`, this
/// fails instead of returning a partial list, keeps both sides of a rename,
/// and reads NUL-separated names so unusual file names are not misparsed.
fn worktree_dirty_paths(root: &Path) -> Result<DirtyPaths, String> {
    let changed = git_bytes(
        root,
        &[
            "diff",
            "--no-ext-diff",
            "--no-renames",
            "--relative",
            "--name-only",
            "-z",
            "HEAD",
            "--",
            ".",
        ],
    )?;
    let untracked = git_bytes(
        root,
        &["ls-files", "-o", "--exclude-standard", "-z", "--", "."],
    )?;
    Ok(DirtyPaths {
        changed: nul_separated_paths(&changed)?,
        untracked: nul_separated_paths(&untracked)?,
    })
}

fn nul_separated_paths(bytes: &[u8]) -> Result<BTreeSet<String>, String> {
    let mut paths = BTreeSet::new();
    for raw in bytes.split(|byte| *byte == 0).filter(|raw| !raw.is_empty()) {
        let relative = String::from_utf8(raw.to_vec()).map_err(|_| {
            format!(
                "Cannot safely integrate {}: its name is not valid UTF-8. The worker worktree was kept.",
                String::from_utf8_lossy(raw)
            )
        })?;
        // `ls-files -o` lists a nested repository as its directory.
        if relative.ends_with('/') {
            return Err(format!(
                "Cannot safely integrate {relative}: it is a nested git repository. The worker worktree was kept."
            ));
        }
        if state_blob_path(Path::new(""), &relative).is_err() {
            return Err(format!(
                "Cannot safely integrate {relative}: git reported an invalid path. The worker worktree was kept."
            ));
        }
        paths.insert(relative);
    }
    Ok(paths)
}

/// Read an isolated worker's delta from its own worktree. Each path starts
/// from its seed snapshot, or from HEAD when the worktree was clean there,
/// so shell edits and completion-only tool events are all included.
fn isolated_worker_delta(
    dir: &Path,
    root: &Path,
    manifest: &Manifest,
) -> Result<WorkerDelta, String> {
    let dirty = worktree_dirty_paths(root)?;
    let ignorecase = ignores_case(root);
    // After `git mv Foo.ts foo.ts` git reports both names. The HEAD name
    // carries the content change and the rename covers the new spelling.
    let twins = if ignorecase {
        staged_case_twins(root, &dirty.changed)
    } else {
        BTreeMap::new()
    };
    // Tool events add gitignored files the worker edited on purpose. Other
    // ignored files are reported apart in `ignored`.
    let candidates: BTreeSet<&String> = manifest
        .seed
        .keys()
        .chain(&dirty.changed)
        .chain(&dirty.untracked)
        .chain(&manifest.touched)
        .filter(|relative| !twins.contains_key(*relative))
        .collect();
    let listed_inside = |relative: &str| {
        let prefix = format!("{relative}/");
        let seed = manifest.seed.keys().map(String::as_str);
        let changed = dirty.changed.iter().map(String::as_str);
        let untracked = dirty.untracked.iter().map(String::as_str);
        seed.chain(changed)
            .chain(untracked)
            .any(|path| path.starts_with(&prefix))
    };
    let mut changes = Vec::new();
    for relative in candidates {
        if state_blob_path(Path::new(""), relative).is_err() {
            return Err(format!(
                "Cannot safely integrate {relative}: the path is invalid. The worker worktree was kept."
            ));
        }
        if path_contains_symlink(root, relative) {
            return Err(format!(
                "Cannot safely integrate {relative}: the worker path contains a symbolic link. The worker worktree was kept."
            ));
        }
        if let Some(stat) = manifest.seed_stat.get(relative) {
            if file_stat(&root.join(relative)).as_ref() == Some(stat) {
                continue;
            }
        }
        let only_touched = !manifest.seed.contains_key(relative)
            && !dirty.changed.contains(relative)
            && !dirty.untracked.contains(relative);
        if only_touched && root.join(relative).is_dir() {
            // A tool event that named a directory. Only files are integrated.
            continue;
        }
        let mut head_before = false;
        let before = if let Some(kind) = manifest.seed.get(relative) {
            stored_snapshot_at(&dir.join("seed"), relative, *kind)
        } else if dirty.changed.contains(relative) || !dirty.untracked.contains(relative) {
            // Changed against HEAD, or a touched ignored file, which is
            // Missing here unless HEAD has it.
            head_before = true;
            head_snapshot(root, relative)?
        } else {
            // Only untracked: the path is not in the index, so HEAD cannot
            // contain it or `git diff HEAD` would report it as deleted.
            (FileState::Missing, None)
        };
        let mut after = worktree_snapshot(root, relative);
        if after.0 == FileState::Skipped
            && is_real_dir(&root.join(relative))
            && listed_inside(relative)
        {
            // A file replaced by a folder. Git lists the folder's files as
            // their own candidates, so the file itself is gone.
            after = (FileState::Missing, None);
        }
        if before.0 == FileState::Skipped || after.0 == FileState::Skipped {
            return Err(format!(
                "Cannot safely integrate {relative}: this file type or size cannot be checkpointed. The worker worktree was kept."
            ));
        }
        if before != after {
            changes.push(WorkerChange {
                relative: relative.clone(),
                before,
                after,
                head_before,
            });
        }
    }
    let (renames, unconfirmed) = if ignorecase {
        case_renames(root, twins.values())?
    } else {
        (Vec::new(), Vec::new())
    };
    Ok(WorkerDelta {
        changes,
        renames,
        unconfirmed,
        ignored: ignored_listing(root)?
            .into_iter()
            .filter(|entry| {
                !manifest.seed_ignored.contains(entry)
                    && !manifest.touched.contains(entry.trim_end_matches('/'))
            })
            .collect(),
    })
}

/// Ignored files, and wholly ignored folders as `dir/`, that are not in the
/// index. Git does not look inside those folders or nested repositories.
fn ignored_listing(root: &Path) -> Result<BTreeSet<String>, String> {
    let bytes = git_bytes(
        root,
        &[
            "ls-files",
            "-o",
            "-i",
            "--exclude-standard",
            "--directory",
            "-z",
            "--",
            ".",
        ],
    )?;
    Ok(bytes
        .split(|byte| *byte == 0)
        .filter(|raw| !raw.is_empty())
        .map(|raw| String::from_utf8_lossy(raw).into_owned())
        .collect())
}

/// Whether the lead already has a worker-created ignored entry. A folder
/// only needs to exist, since folders such as `node_modules/` are rebuilt
/// per checkout. A file must match byte for byte.
fn lead_has_same_entry(worker: &Path, lead: &Path, entry: &str) -> bool {
    if let Some(folder) = entry.strip_suffix('/') {
        return state_blob_path(Path::new(""), folder).is_ok() && lead.join(folder).is_dir();
    }
    if state_blob_path(Path::new(""), entry).is_err() {
        return false;
    }
    let (ours, theirs) = (worker.join(entry), lead.join(entry));
    let (Ok(our_meta), Ok(their_meta)) = (
        std::fs::symlink_metadata(&ours),
        std::fs::symlink_metadata(&theirs),
    ) else {
        return false;
    };
    if our_meta.file_type().is_symlink() || their_meta.file_type().is_symlink() {
        return our_meta.file_type().is_symlink()
            && their_meta.file_type().is_symlink()
            && matches!(
                (std::fs::read_link(&ours), std::fs::read_link(&theirs)),
                (Ok(a), Ok(b)) if a == b
            );
    }
    our_meta.is_file()
        && their_meta.is_file()
        && our_meta.len() == their_meta.len()
        && same_bytes(&ours, &theirs)
}

/// Compare two files in chunks, so a large ignored file is never read whole.
fn same_bytes(a: &Path, b: &Path) -> bool {
    use std::io::Read;
    fn fill(file: &mut std::fs::File, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut total = 0;
        while total < buf.len() {
            match file.read(&mut buf[total..])? {
                0 => break,
                read => total += read,
            }
        }
        Ok(total)
    }
    let (Ok(mut a), Ok(mut b)) = (std::fs::File::open(a), std::fs::File::open(b)) else {
        return false;
    };
    let mut left = vec![0u8; 64 * 1024];
    let mut right = vec![0u8; 64 * 1024];
    loop {
        let (Ok(read_left), Ok(read_right)) = (fill(&mut a, &mut left), fill(&mut b, &mut right))
        else {
            return false;
        };
        if left[..read_left] != right[..read_right] {
            return false;
        }
        if read_left == 0 {
            return true;
        }
    }
}

/// Paths git reports as added whose HEAD twin, differing only in case, is
/// also reported and is the same file on disk, as after
/// `git mv Foo.ts foo.ts`. Maps each added path to its HEAD name.
fn staged_case_twins(root: &Path, changed: &BTreeSet<String>) -> BTreeMap<String, String> {
    let mut folded: HashMap<String, Vec<&String>> = HashMap::new();
    for relative in changed {
        folded
            .entry(relative.to_lowercase())
            .or_default()
            .push(relative);
    }
    let mut twins = BTreeMap::new();
    for group in folded.values().filter(|group| group.len() == 2) {
        let (head, added): (Vec<&String>, Vec<&String>) =
            group.iter().partition(|relative| in_head(root, relative));
        if let ([head], [added]) = (head.as_slice(), added.as_slice()) {
            if same_file(&root.join(head), &root.join(added)) {
                twins.insert((*added).clone(), (*head).clone());
            }
        }
    }
    twins
}

#[cfg(unix)]
fn same_file(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::symlink_metadata(a), std::fs::symlink_metadata(b)) {
        (Ok(a), Ok(b)) => a.is_file() && a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}

#[cfg(not(unix))]
fn same_file(a: &Path, b: &Path) -> bool {
    matches!(
        (std::fs::canonicalize(a), std::fs::canonicalize(b)),
        (Ok(a), Ok(b)) if a == b
    )
}

/// Case-only renames in a worktree on a case-insensitive file system, where
/// `mv Foo.ts foo.ts` leaves git's output unchanged. Each index path, plus
/// the HEAD names in `extra`, is resolved to its spelling on disk one
/// component at a time, reading each folder once. A renamed folder is one
/// rename, not one per file inside. The second list holds folder renames
/// seen only on paths that no longer resolve.
fn case_renames<'a>(
    root: &Path,
    extra: impl Iterator<Item = &'a String>,
) -> Result<(Vec<CheckpointRename>, Vec<CheckpointRename>), String> {
    let listing = git_bytes(root, &["ls-files", "-z"])?;
    let mut folders: HashMap<PathBuf, Option<FolderNames>> = HashMap::new();
    let mut found = BTreeMap::new();
    let mut partial = BTreeMap::new();
    let mut paths: Vec<&str> = listing
        .split(|byte| *byte == 0)
        .filter(|raw| !raw.is_empty())
        .filter_map(|raw| std::str::from_utf8(raw).ok())
        .collect();
    for path in extra {
        paths.push(path);
    }
    for relative in paths {
        let mut disk = root.to_path_buf();
        let (mut from, mut to) = (String::new(), String::new());
        let mut renamed = Vec::new();
        let mut resolved = true;
        for part in relative.split('/') {
            let names = folders
                .entry(disk.clone())
                .or_insert_with(|| FolderNames::read(&disk));
            let Some(actual) = names.as_ref().and_then(|names| names.spelling(part)) else {
                // Deleted, so git already reports it.
                resolved = false;
                break;
            };
            if !from.is_empty() {
                from.push('/');
                to.push('/');
            }
            from.push_str(part);
            to.push_str(&actual);
            if actual != part {
                renamed.push((from.clone(), to.clone()));
            }
            disk.push(&actual);
        }
        if resolved {
            found.extend(renamed);
        } else {
            partial.extend(renamed);
        }
    }
    let sorted = |pairs: BTreeMap<String, String>| {
        let mut renames: Vec<CheckpointRename> = pairs
            .into_iter()
            .map(|(from, to)| CheckpointRename { from, to })
            .collect();
        renames.sort_by_key(|rename| rename.from.matches('/').count());
        renames
    };
    partial.retain(|from, _| !found.contains_key(from));
    Ok((sorted(found), sorted(partial)))
}

fn ignores_case(root: &Path) -> bool {
    git_bytes(root, &["config", "--bool", "core.ignorecase"])
        .map(|out| out.trim_ascii() == b"true")
        .unwrap_or(false)
}

/// The UTF-8 names in one folder, indexed for case-insensitive lookup.
struct FolderNames {
    exact: HashSet<String>,
    folded: HashMap<String, Vec<String>>,
}

impl FolderNames {
    fn read(dir: &Path) -> Option<Self> {
        let mut names = Self {
            exact: HashSet::new(),
            folded: HashMap::new(),
        };
        for entry in std::fs::read_dir(dir).ok()? {
            let Ok(name) = entry.ok()?.file_name().into_string() else {
                continue;
            };
            names
                .folded
                .entry(name.to_lowercase())
                .or_default()
                .push(name.clone());
            names.exact.insert(name);
        }
        Some(names)
    }

    /// The on-disk spelling of `name`, when exactly one entry matches it.
    fn spelling(&self, name: &str) -> Option<String> {
        if self.exact.contains(name) {
            return Some(name.to_string());
        }
        match self.folded.get(&name.to_lowercase())?.as_slice() {
            [only] => Some(only.clone()),
            _ => None,
        }
    }
}

/// The on-disk spelling of `name` inside `dir`, or None when it is absent.
fn disk_spelling(dir: &Path, name: &str) -> Option<String> {
    FolderNames::read(dir)?.spelling(name)
}

/// `relative` after the deepest rename of it or one of its folders.
fn renamed_path(relative: &str, renames: &[CheckpointRename]) -> String {
    renames
        .iter()
        .filter(|rename| {
            relative
                .strip_prefix(rename.from.as_str())
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
        })
        .max_by_key(|rename| rename.from.len())
        .map(|rename| format!("{}{}", rename.to, &relative[rename.from.len()..]))
        .unwrap_or_else(|| relative.to_string())
}

/// Split a relative path into its parent (empty at the top) and last name.
fn split_last(relative: &str) -> (&str, &str) {
    relative.rsplit_once('/').unwrap_or(("", relative))
}

fn is_real_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir())
}

/// The HEAD version of a file in the same form as `worktree_snapshot`:
/// checkout filters applied, the size limit enforced, and a unix mode.
fn head_snapshot(root: &Path, relative: &str) -> Result<(FileState, Option<u32>), String> {
    let listing = git_bytes(root, &["ls-tree", "-z", "-l", "HEAD", "--", relative])?;
    let entry = listing
        .split(|byte| *byte == 0)
        .filter_map(|raw| {
            let text = std::str::from_utf8(raw).ok()?;
            let (meta, path) = text.split_once('\t')?;
            (path == relative).then_some(meta)
        })
        .next();
    let Some(meta) = entry else {
        return Ok((FileState::Missing, None));
    };
    let fields: Vec<&str> = meta.split_whitespace().collect();
    let [mode, "blob", _, size] = fields.as_slice() else {
        return Ok((FileState::Skipped, None));
    };
    let executable = match *mode {
        "100644" => false,
        "100755" => true,
        _ => return Ok((FileState::Skipped, None)),
    };
    if size
        .parse::<u64>()
        .map_or(true, |size| size > MAX_TEXT_FILE_BYTES)
    {
        return Ok((FileState::Skipped, None));
    }
    let bytes = git_bytes(
        root,
        &["cat-file", "--filters", &format!("HEAD:./{relative}")],
    )?;
    if bytes.len() as u64 > MAX_TEXT_FILE_BYTES {
        return Ok((FileState::Skipped, None));
    }
    Ok((
        FileState::Contents(bytes),
        head_file_mode(&root.join(relative), executable),
    ))
}

/// Git records only the executable bit. Reuse the live file's mode when that
/// bit agrees, so an unchanged checkout compares equal under any umask.
#[cfg(unix)]
fn head_file_mode(path: &Path, executable: bool) -> Option<u32> {
    match file_mode(path) {
        Some(mode) if (mode & 0o100 != 0) == executable => Some(mode),
        _ if executable => Some(0o755),
        _ => Some(0o644),
    }
}

#[cfg(not(unix))]
fn head_file_mode(_path: &Path, _executable: bool) -> Option<u32> {
    None
}

/// Compare two file states. With `loose_mode`, modes match when their
/// executable bits agree, because that bit is all git records.
fn same_state(
    a: &(FileState, Option<u32>),
    b: &(FileState, Option<u32>),
    loose_mode: bool,
) -> bool {
    if a.0 != b.0 {
        return false;
    }
    match (a.1, b.1) {
        (Some(x), Some(y)) if loose_mode => (x & 0o111 != 0) == (y & 0o111 != 0),
        (x, y) => x == y,
    }
}

/// Each write scope as components below `root`. Scopes are absolute paths
/// in the worker checkout and may name files that do not exist yet. Scopes
/// are compared lexically and never resolved through symlinks, because the
/// worker controls every link in its checkout.
fn scope_parts(root: &Path, scopes: &[String]) -> Vec<Vec<String>> {
    let canonical_root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let lexical_root = normalize_lexically(root);
    let mut allowed: Vec<Vec<String>> = Vec::new();
    for scope in scopes {
        let trimmed = scope.trim();
        if trimmed.is_empty() {
            continue;
        }
        let expanded = expand_home(trimmed);
        let path = if expanded.is_absolute() {
            expanded
        } else {
            root.join(expanded)
        };
        let lexical = normalize_lexically(&path);
        for parts in [
            components_under(&lexical, &lexical_root),
            components_under(&lexical, &canonical_root),
        ]
        .into_iter()
        .flatten()
        {
            // A scope reached through a symlink, such as `src/feature`
            // pointing at `..`, could name the whole repository.
            if !parts.is_empty() && path_contains_symlink(root, &parts.join("/")) {
                continue;
            }
            allowed.push(parts);
        }
    }
    allowed
}

/// Whether `relative` is equal to or inside any scope from `scope_parts`.
fn in_scopes(allowed: &[Vec<String>], relative: &str) -> bool {
    let parts: Vec<&str> = relative.split('/').collect();
    allowed.iter().any(|scope| {
        scope.len() <= parts.len() && scope.iter().zip(&parts).all(|(a, b)| same_part(a, b))
    })
}

/// The components of `path` below `root`, or None when it is not inside.
fn components_under(path: &Path, root: &Path) -> Option<Vec<String>> {
    let mut parts = path.components();
    for root_part in root.components() {
        let part = parts.next()?;
        if !same_part(
            &part.as_os_str().to_string_lossy(),
            &root_part.as_os_str().to_string_lossy(),
        ) {
            return None;
        }
    }
    Some(
        parts
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect(),
    )
}

/// The frontend lowercases scopes on Windows, where paths are
/// case-insensitive.
#[cfg(windows)]
fn same_part(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

#[cfg(not(windows))]
fn same_part(a: &str, b: &str) -> bool {
    a == b
}

fn normalize_lexically(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

fn lead_changed(relative: &str) -> String {
    format!(
        "Cannot integrate {relative}: the lead checkout changed since this worker started. The worker worktree was kept."
    )
}

fn still_changing(relative: &str) -> String {
    format!(
        "The worker is still changing {relative}. Stop it and accept again. The worker worktree was kept."
    )
}

/// How a lead path compares with one change.
enum Target {
    After,
    Before,
    Other,
}

/// Compare the lead's file at `path` with a change. `path` is the change's
/// path after any case renames.
fn lead_target(
    root: &Path,
    path: &str,
    change: &WorkerChange,
    loose_mode: bool,
) -> Result<Target, String> {
    // A folder where the worker deleted a file has no file there either.
    if loose_mode && change.after.0 == FileState::Missing && is_real_dir(&root.join(path)) {
        return Ok(Target::After);
    }
    let target = worktree_snapshot(root, path);
    if same_state(&target, &change.after, loose_mode) {
        return Ok(Target::After);
    }
    if same_state(&target, &change.before, loose_mode) {
        return Ok(Target::Before);
    }
    // A before state read from HEAD used the worker's attributes and config,
    // such as core.autocrlf, so an unmodified lead file can still differ in
    // its bytes. Ask the lead's own git whether the file matches HEAD.
    if change.head_before
        && matches!(change.before.0, FileState::Contents(_))
        && matches!(target.0, FileState::Contents(_))
        && lead_unmodified(root, path)?
    {
        return Ok(Target::Before);
    }
    Ok(Target::Other)
}

/// True when the lead's git reports no staged or unstaged change to the
/// tracked file `relative`. Pathspecs are literal in `git_bytes`.
fn lead_unmodified(root: &Path, relative: &str) -> Result<bool, String> {
    let status = git_bytes(
        root,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=no",
            "--",
            relative,
        ],
    )?;
    Ok(status.is_empty())
}

/// True when every file under the lead folder `relative`, including
/// untracked and ignored ones, is deleted by this apply. The folder is then
/// pruned before a file takes its place. An empty folder would stay behind,
/// so it does not count.
fn folder_is_being_deleted(root: &Path, relative: &str, deleted: &BTreeSet<&str>) -> bool {
    let Ok(entries) = std::fs::read_dir(root.join(relative)) else {
        return false;
    };
    let mut any = false;
    for entry in entries {
        let Ok(entry) = entry else {
            return false;
        };
        let (Ok(name), Ok(kind)) = (entry.file_name().into_string(), entry.file_type()) else {
            return false;
        };
        let child = format!("{relative}/{name}");
        let gone = if kind.is_dir() {
            folder_is_being_deleted(root, &child, deleted)
        } else {
            deleted.contains(child.as_str())
        };
        if !gone {
            return false;
        }
        any = true;
    }
    any
}

/// Check every change and rename against the lead before anything is
/// written, and count those already applied.
fn preflight(
    root: &Path,
    changes: &[WorkerChange],
    renames: &[CheckpointRename],
    loose_mode: bool,
) -> Result<usize, String> {
    let deleted: BTreeSet<&str> = changes
        .iter()
        .filter(|change| change.after.0 == FileState::Missing)
        .map(|change| change.relative.as_str())
        .collect();
    let mut already_applied = 0;
    for change in changes {
        let relative = &change.relative;
        if path_contains_symlink(root, relative) {
            return Err(format!(
                "Cannot integrate {relative}: the target path contains a symbolic link. The worker worktree was kept."
            ));
        }
        match lead_target(root, relative, change, loose_mode)? {
            Target::After => already_applied += 1,
            Target::Before => {}
            Target::Other
                if loose_mode
                    && change.before.0 == FileState::Missing
                    && is_real_dir(&root.join(relative))
                    && folder_is_being_deleted(root, relative, &deleted) => {}
            Target::Other => return Err(lead_changed(relative)),
        }
    }
    for rename in renames {
        if path_contains_symlink(root, &rename.from) {
            return Err(format!(
                "Cannot integrate {}: the target path contains a symbolic link. The worker worktree was kept.",
                rename.from
            ));
        }
        let (parent, _) = split_last(&rename.from);
        if lead_rename(root, parent, rename, changes)? == RenameState::Applied {
            already_applied += 1;
        }
    }
    Ok(already_applied)
}

/// Folder renames from `unconfirmed` that files written by this apply
/// depend on. A new file may land under the worker's spelling only when
/// the lead's folder is absent, already renamed, or about to be pruned
/// because this apply deletes everything in it. Otherwise there is no way
/// to tell which spelling the lead's remaining files should end up under.
fn unconfirmed_aliases(
    root: &Path,
    unconfirmed: &[CheckpointRename],
    changes: &[WorkerChange],
) -> Result<Vec<CheckpointRename>, String> {
    let deleted: BTreeSet<&str> = changes
        .iter()
        .filter(|change| change.after.0 == FileState::Missing)
        .map(|change| change.relative.as_str())
        .collect();
    let mut aliases = Vec::new();
    for folder in unconfirmed {
        let prefix = format!("{}/", folder.from.to_lowercase());
        let writes_inside = changes.iter().any(|change| {
            change.after.0 != FileState::Missing
                && change.relative.to_lowercase().starts_with(&prefix)
        });
        if !writes_inside {
            continue;
        }
        let (parent, old) = split_last(&folder.from);
        let (_, new) = split_last(&folder.to);
        let safe = match disk_spelling(&root.join(parent), old) {
            None => true,
            Some(name) if name == new => true,
            Some(name) if name == old => folder_is_being_deleted(root, &folder.from, &deleted),
            Some(_) => false,
        };
        if !safe {
            return Err(format!(
                "Cannot tell how the worker renamed {}. The worker worktree was kept.",
                folder.from
            ));
        }
        aliases.push(folder.clone());
    }
    Ok(aliases)
}

#[derive(PartialEq, Eq)]
enum RenameState {
    /// The lead still has the git name.
    Pending,
    Applied,
    /// The lead lacks the path, and this apply creates it at the new
    /// spelling, so no rename is needed.
    Created,
    /// An earlier apply stopped between the two steps of `rename_case`,
    /// leaving the path under its temporary name.
    Resume,
}

/// Where a case rename stands in the lead. `parent` is the lead folder that
/// holds the renamed name, spelled either way.
fn lead_rename(
    root: &Path,
    parent: &str,
    rename: &CheckpointRename,
    changes: &[WorkerChange],
) -> Result<RenameState, String> {
    let (_, old) = split_last(&rename.from);
    let (_, new) = split_last(&rename.to);
    let temp = case_temp(&root.join(parent), new);
    let halfway = std::fs::symlink_metadata(&temp).is_ok();
    match disk_spelling(&root.join(parent), old) {
        Some(name) if name == new && !halfway => Ok(RenameState::Applied),
        Some(name) if name == old && !halfway => Ok(RenameState::Pending),
        None if halfway && resumable(root, rename, &temp, changes) => Ok(RenameState::Resume),
        None if !halfway
            && changes.iter().any(|change| {
                change.before.0 == FileState::Missing
                    && change
                        .relative
                        .strip_prefix(rename.from.as_str())
                        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
            }) =>
        {
            Ok(RenameState::Created)
        }
        _ => Err(lead_changed(&rename.from)),
    }
}

/// Whether a leftover temporary name holds what this rename moved: each
/// file must match its change's before or after state, or HEAD when the
/// file has no content change.
fn resumable(
    root: &Path,
    rename: &CheckpointRename,
    temp: &Path,
    changes: &[WorkerChange],
) -> bool {
    let matches = |relative: &str, file: &Path| {
        if let Some(change) = changes.iter().find(|change| change.relative == relative) {
            let bytes = std::fs::read(file).ok().map(FileState::Contents);
            if bytes.as_ref() == Some(&change.before.0) || bytes.as_ref() == Some(&change.after.0) {
                return true;
            }
        }
        matches_head_blob(root, relative, file)
    };
    let Ok(meta) = std::fs::symlink_metadata(temp) else {
        return false;
    };
    if meta.is_file() {
        return matches(&rename.from, temp);
    }
    if !meta.is_dir() {
        return false;
    }
    // A folder: every file this apply knows under it must match.
    let prefix = format!("{}/", rename.from);
    changes.iter().all(|change| {
        let Some(rest) = change.relative.strip_prefix(&prefix) else {
            return true;
        };
        let file = temp.join(rest);
        !file.exists() || matches(&change.relative, &file)
    })
}

/// True when `file`, cleaned with the attributes for `relative`, hashes to
/// the blob HEAD records at `relative`.
fn matches_head_blob(root: &Path, relative: &str, file: &Path) -> bool {
    let file = file.to_string_lossy();
    let path = format!("--path={relative}");
    match (
        git_bytes(root, &["hash-object", &path, "--", &file]),
        git_bytes(
            root,
            &["rev-parse", "--verify", &format!("HEAD:{relative}")],
        ),
    ) {
        (Ok(hashed), Ok(head)) => hashed.trim_ascii() == head.trim_ascii(),
        _ => false,
    }
}

/// Write each change into the lead checkout: deletions first, deepest
/// first, then case renames, then content. Every target is checked again
/// just before its write, so an edit made after the preflight stops the
/// run. Files already written stay written and a retry skips them. With
/// `worker`, each worker file must still hold the state being applied, and
/// folders emptied by a deletion are removed.
fn write_changes(
    root: &Path,
    changes: &[WorkerChange],
    renames: &[CheckpointRename],
    aliases: &[CheckpointRename],
    loose_mode: bool,
    worker: Option<&Path>,
) -> Result<(), String> {
    let mut deletions: Vec<&WorkerChange> = changes
        .iter()
        .filter(|change| change.after.0 == FileState::Missing)
        .collect();
    deletions.sort_by(|a, b| {
        let depth = |change: &WorkerChange| change.relative.matches('/').count();
        depth(b)
            .cmp(&depth(a))
            .then_with(|| b.relative.cmp(&a.relative))
    });
    for change in deletions {
        let relative = &change.relative;
        match lead_target(root, relative, change, loose_mode)? {
            Target::After => {}
            Target::Before => {
                check_worker(worker, change)?;
                write_state(root, relative, FileState::Missing, None, loose_mode)?;
            }
            Target::Other => return Err(lead_changed(relative)),
        }
        if worker.is_some() {
            prune_empty_parents(root, relative);
        }
    }

    for rename in renames {
        let (_, old) = split_last(&rename.from);
        let (new_parent, new) = split_last(&rename.to);
        // Shallower renames ran first, so the parent has its new spelling.
        let state = lead_rename(root, new_parent, rename, changes)?;
        if matches!(state, RenameState::Applied | RenameState::Created) {
            continue;
        }
        if let Some(worker) = worker {
            if disk_spelling(&worker.join(new_parent), new).as_deref() != Some(new) {
                return Err(still_changing(&rename.to));
            }
        }
        let parent = root.join(new_parent);
        if state == RenameState::Resume {
            std::fs::rename(case_temp(&parent, new), parent.join(new))
                .map_err(|e| e.to_string())?;
        } else {
            rename_case(&parent, old, new)?;
        }
    }

    let spellings: Vec<CheckpointRename> = renames.iter().chain(aliases).cloned().collect();
    for change in changes
        .iter()
        .filter(|change| change.after.0 != FileState::Missing)
    {
        let path = renamed_path(&change.relative, &spellings);
        match lead_target(root, &path, change, loose_mode)? {
            Target::After => continue,
            Target::Before => {}
            Target::Other => return Err(lead_changed(&change.relative)),
        }
        check_worker(worker, change)?;
        let (state, mode) = change.after.clone();
        write_state(root, &path, state, mode, loose_mode)?;
    }
    Ok(())
}

/// The worker's live state for one change. A folder reads as Missing, as
/// in `isolated_worker_delta`.
fn check_worker(worker: Option<&Path>, change: &WorkerChange) -> Result<(), String> {
    let Some(worker) = worker else {
        return Ok(());
    };
    let live = if is_real_dir(&worker.join(&change.relative)) {
        (FileState::Missing, None)
    } else {
        worktree_snapshot(worker, &change.relative)
    };
    if live != change.after {
        return Err(still_changing(&change.relative));
    }
    Ok(())
}

/// Read the worker's delta again after the writes. A path that appeared,
/// vanished, or changed means the worker kept working during the apply.
fn verify_worker_settled(
    dir: &Path,
    worker: &Path,
    manifest: &Manifest,
    applied: &WorkerDelta,
) -> Result<(), String> {
    let now = isolated_worker_delta(dir, worker, manifest)?;
    let states = |delta: &WorkerDelta| -> BTreeMap<String, (FileState, Option<u32>)> {
        delta
            .changes
            .iter()
            .map(|change| (change.relative.clone(), change.after.clone()))
            .collect()
    };
    let (was, is) = (states(applied), states(&now));
    if let Some(relative) = was
        .keys()
        .chain(is.keys())
        .find(|relative| was.get(*relative) != is.get(*relative))
    {
        return Err(still_changing(relative));
    }
    if let Some(rename) = applied
        .renames
        .iter()
        .chain(&now.renames)
        .find(|rename| !applied.renames.contains(rename) || !now.renames.contains(rename))
    {
        return Err(still_changing(&rename.to));
    }
    if let Some(entry) = applied
        .ignored
        .iter()
        .chain(&now.ignored)
        .find(|entry| !applied.ignored.contains(entry) || !now.ignored.contains(entry))
    {
        return Err(still_changing(entry));
    }
    Ok(())
}

/// Remove the folders above `relative` that are now empty, up to but not
/// including `root`.
fn prune_empty_parents(root: &Path, relative: &str) {
    let mut current = Path::new(relative).parent();
    while let Some(parent) = current.filter(|parent| !parent.as_os_str().is_empty()) {
        if std::fs::remove_dir(root.join(parent)).is_err() {
            break;
        }
        current = parent.parent();
    }
}

/// Rename `old` to `new` inside `parent`, where the names differ only in
/// case. A case-insensitive file system may treat a direct rename as a
/// no-op, so go through a temporary name. The name is fixed, so a retry
/// after a crash between the two steps finds it and finishes the rename.
fn rename_case(parent: &Path, old: &str, new: &str) -> Result<(), String> {
    let temp = case_temp(parent, new);
    if std::fs::symlink_metadata(&temp).is_ok() {
        return Err(format!("{} already exists", temp.display()));
    }
    std::fs::rename(parent.join(old), &temp).map_err(|e| e.to_string())?;
    if let Err(error) = std::fs::rename(&temp, parent.join(new)) {
        let _ = std::fs::rename(&temp, parent.join(old));
        return Err(error.to_string());
    }
    Ok(())
}

static TEMP_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The fixed temporary name `rename_case` uses for `new` in `dir`.
fn case_temp(dir: &Path, new: &str) -> PathBuf {
    dir.join(format!(".{}.monocode-case.tmp", short_name(new, 200)))
}

/// At most `limit` bytes of `name`, so a temporary name stays within file
/// name length limits.
fn short_name(name: &str, limit: usize) -> &str {
    let mut end = name.len().min(limit);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    &name[..end]
}

/// A unique hidden name next to `name` in `dir`.
fn temp_sibling(dir: &Path, name: &str) -> PathBuf {
    let short = short_name(name, 64);
    let seq = TEMP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    dir.join(format!(
        ".{short}.monocode-{}-{seq}.tmp",
        std::process::id()
    ))
}

/// Write one file state at exactly `relative`. The path is validated but
/// never rewritten, so names with spaces or " => " stay literal. With
/// `exec_only`, the target keeps its own permission bits and takes only
/// the executable bit from `mode`.
fn write_state(
    root: &Path,
    relative: &str,
    state: FileState,
    mode: Option<u32>,
    exec_only: bool,
) -> Result<(), String> {
    state_blob_path(root, relative)?;
    if path_contains_symlink(root, relative) {
        return Err(format!("Cannot write through symbolic link {relative}"));
    }
    match state {
        FileState::Contents(bytes) => {
            let path = root.join(relative);
            let mode = if exec_only {
                exec_mode(file_mode(&path), mode)
            } else {
                mode
            };
            write_file_atomic(&path, &bytes, mode)
        }
        FileState::Missing => remove_worktree(root, relative),
        FileState::Skipped => Err(format!("Cannot write unsupported file {relative}")),
    }
}

/// Keep `existing` permission bits (0o644 for a new file) and set or clear
/// only the executable bits to match `worker`. Set bits follow the read
/// bits, so 0o640 becomes 0o750.
fn exec_mode(existing: Option<u32>, worker: Option<u32>) -> Option<u32> {
    let executable = worker? & 0o111 != 0;
    Some(match existing {
        Some(mode) if executable => mode | ((mode & 0o444) >> 2),
        Some(mode) => mode & !0o111,
        None if executable => 0o755,
        None => 0o644,
    })
}

fn stored_snapshot(
    dir: &Path,
    relative: &str,
    kind: SnapshotKind,
    after: bool,
) -> (FileState, Option<u32>) {
    let blob_root = if after {
        dir.join("after")
    } else {
        dir.join("files")
    };
    stored_snapshot_at(&blob_root, relative, kind)
}

fn stored_snapshot_at(
    blob_root: &Path,
    relative: &str,
    kind: SnapshotKind,
) -> (FileState, Option<u32>) {
    (
        read_snapshot_at(blob_root, relative, kind),
        snapshot_mode(blob_root, relative, kind),
    )
}

fn worktree_snapshot(root: &Path, relative: &str) -> (FileState, Option<u32>) {
    (
        read_worktree(root, relative),
        file_mode(&root.join(relative)),
    )
}

fn path_contains_symlink(root: &Path, relative: &str) -> bool {
    let mut current = root.to_path_buf();
    for part in relative.split('/') {
        current.push(part);
        match std::fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() => return true,
            Ok(_) => {}
            // A missing component, or a file where a folder should be,
            // means nothing below it exists to follow.
            Err(error) if is_missing(&error) => return false,
            Err(_) => return true,
        }
    }
    false
}

fn git_head(root: &Path) -> Result<Vec<u8>, String> {
    git_bytes(root, &["rev-parse", "--verify", "HEAD"])
}

/// Raw stdout of a git command, or its stderr as the error. Pathspecs are
/// literal so file names with glob characters match only themselves.
fn git_bytes(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let mut command = Command::new("git");
    crate::hide_window_console(&mut command);
    let output = command
        .arg("--no-pager")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_LITERAL_PATHSPECS", "1")
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(output.stdout)
}

fn diff_from_manifest(
    dir: &Path,
    root: &Path,
    manifest: &Manifest,
    foreign_touched: &HashSet<String>,
) -> CheckpointStatus {
    diff_from_manifest_with(
        &git_diff_files_for(root),
        dir,
        root,
        manifest,
        foreign_touched,
    )
}

fn diff_from_manifest_with(
    index: &GitDiffIndex,
    dir: &Path,
    root: &Path,
    manifest: &Manifest,
    foreign_touched: &HashSet<String>,
) -> CheckpointStatus {
    let by_relative: BTreeMap<&str, &GitChangedFile> = index
        .files
        .iter()
        .map(|file| (file.relative.as_str(), file))
        .collect();
    let git_dirty: HashSet<&str> = by_relative.keys().copied().collect();
    let mut files = Vec::new();

    for relative in &manifest.touched {
        // Without a tool-start snapshot there is no trustworthy session
        // boundary. Never guess from the shared working tree.
        if !manifest.prepared.contains(relative) {
            continue;
        }
        if session_snapshot_differs(dir, manifest, relative) == Some(false) {
            continue;
        }
        if !file_differs(dir, root, manifest, relative, &git_dirty) {
            continue;
        }
        // Review is always scoped to this session's captured before/after
        // snapshots. A foreign claim can make restoring the file unsafe, but
        // it does not make this session's recorded diff or counts inexact.
        let exact = !manifest.diverged.contains(relative);
        let undoable = exact
            && !foreign_touched.contains(relative)
            && after_matches_worktree(dir, root, manifest, relative);
        let session_change = manifest.stats.get(relative).map(|stats| {
            let additions = if exact { stats.additions } else { 0 };
            let deletions = if exact { stats.deletions } else { 0 };
            (stats.status.clone(), additions, deletions)
        });
        files.push(describe_change(
            root,
            relative,
            by_relative.get(relative.as_str()).copied(),
            exact,
            undoable,
            session_change,
        ));
    }

    files.sort_by(|a, b| a.relative.cmp(&b.relative));
    CheckpointStatus { files }
}

fn session_snapshot_differs(dir: &Path, manifest: &Manifest, relative: &str) -> Option<bool> {
    let before = manifest.files.get(relative).copied()?;
    let after = manifest.after.get(relative).copied()?;
    Some(read_snapshot(dir, relative, before) != read_after_snapshot(dir, relative, after))
}

#[cfg(test)]
fn stats_from_status(status: &CheckpointStatus) -> GitDiffStats {
    let mut additions = 0i64;
    let mut deletions = 0i64;
    for file in &status.files {
        additions += file.additions;
        deletions += file.deletions;
    }
    GitDiffStats {
        files: status.files.len() as i64,
        additions,
        deletions,
    }
}

fn file_differs(
    dir: &Path,
    root: &Path,
    manifest: &Manifest,
    relative: &str,
    git_dirty: &HashSet<&str>,
) -> bool {
    // Once a tracked path is clean against HEAD, its session change was
    // committed (or otherwise resolved) and no longer needs review.
    if !git_dirty.contains(relative)
        && (manifest.tracked.contains(relative) || in_head(root, relative))
    {
        return false;
    }
    match manifest.files.get(relative) {
        Some(SnapshotKind::Skipped) => false,
        Some(kind) => read_worktree(root, relative) != read_snapshot(dir, relative, *kind),
        None => {
            git_dirty.contains(relative)
                || (root.join(relative).is_file() && !in_head(root, relative))
        }
    }
}

fn describe_change(
    root: &Path,
    relative: &str,
    git: Option<&GitChangedFile>,
    exact: bool,
    undoable: bool,
    session_change: Option<(String, i64, i64)>,
) -> CheckpointFile {
    if let Some((status, additions, deletions)) = session_change {
        return CheckpointFile {
            path: path_to_js(&root.join(relative)),
            relative: relative.to_string(),
            status,
            additions,
            deletions,
            exact,
            undoable,
        };
    }
    if let Some(file) = git {
        return CheckpointFile {
            path: file.path.clone(),
            relative: file.relative.clone(),
            status: file.status.clone(),
            additions: file.additions,
            deletions: file.deletions,
            exact,
            undoable,
        };
    }
    let abs = root.join(relative);
    let status = if !abs.exists() { "deleted" } else { "modified" };
    CheckpointFile {
        path: path_to_js(&abs),
        relative: relative.to_string(),
        status: status.into(),
        additions: 0,
        deletions: 0,
        exact,
        undoable,
    }
}

fn calculate_session_stats(dir: &Path, manifest: &Manifest, relative: &str) -> Option<ChangeStats> {
    let before = manifest.files.get(relative).copied()?;
    let after = manifest.after.get(relative).copied()?;
    if before == SnapshotKind::Skipped || after == SnapshotKind::Skipped {
        return None;
    }
    let before_path = state_blob_path(&dir.join("files"), relative).ok()?;
    let after_path = state_blob_path(&dir.join("after"), relative).ok()?;
    let (additions, deletions) = diff_numstat(&before_path, &after_path)?;
    let status = match (before, after) {
        (SnapshotKind::Missing, SnapshotKind::Missing) => "modified",
        (SnapshotKind::Missing, _) => "added",
        (_, SnapshotKind::Missing) => "deleted",
        _ => "modified",
    };
    Some(ChangeStats {
        status: status.into(),
        additions,
        deletions,
    })
}

fn diff_numstat(before: &Path, after: &Path) -> Option<(i64, i64)> {
    let mut cmd = Command::new("git");
    crate::hide_window_console(&mut cmd);
    let output = cmd
        .args(["diff", "--no-index", "--no-ext-diff", "--numstat", "--"])
        .arg(before)
        .arg(after)
        .output()
        .ok()?;
    if !output.status.success() && output.status.code() != Some(1) {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut fields = text.lines().next()?.split('\t');
    let additions = fields.next()?.parse().ok()?;
    let deletions = fields.next()?.parse().ok()?;
    Some((additions, deletions))
}

fn after_matches_worktree(dir: &Path, root: &Path, manifest: &Manifest, relative: &str) -> bool {
    let Some(kind) = manifest.after.get(relative).copied() else {
        return false;
    };
    read_worktree(root, relative) == read_after_snapshot(dir, relative, kind)
}

fn release_path(manifest: &mut Manifest, relative: &str) {
    manifest.files.remove(relative);
    manifest.touched.remove(relative);
    manifest.tracked.remove(relative);
    manifest.prepared.remove(relative);
    manifest.after.remove(relative);
    manifest.stats.remove(relative);
    manifest.diverged.remove(relative);
}

/// Drop an isolated session's tool attribution and review state while
/// keeping `isolated`, `seed`, and the seed's entries in `files`.
fn clear_review(dir: &Path, manifest: &mut Manifest) {
    let files_root = dir.join("files");
    let seed = &manifest.seed;
    manifest.files.retain(|relative, _| {
        let keep = seed.contains_key(relative);
        if !keep {
            if let Ok(blob) = state_blob_path(&files_root, relative) {
                let _ = std::fs::remove_file(blob);
            }
        }
        keep
    });
    manifest
        .tracked
        .retain(|relative| seed.contains_key(relative));
    manifest.touched.clear();
    manifest.prepared.clear();
    manifest.after.clear();
    manifest.stats.clear();
    manifest.diverged.clear();
    let _ = std::fs::remove_dir_all(dir.join("after"));
}

fn restore_one(dir: &Path, root: &Path, manifest: &Manifest, relative: &str) -> Result<(), String> {
    let relative = resolve_repo_path(root, relative)?;
    match manifest.files.get(&relative) {
        Some(SnapshotKind::Skipped) => Ok(()),
        Some(kind) => restore_snapshot(dir, root, &relative, *kind),
        None => revert_new_change(root, &relative),
    }
}

fn restore_snapshot(
    dir: &Path,
    root: &Path,
    relative: &str,
    kind: SnapshotKind,
) -> Result<(), String> {
    match kind {
        SnapshotKind::Skipped => Ok(()),
        SnapshotKind::Missing => {
            let _ = git_checked(root, &["reset", "-q", "HEAD", "--", relative]);
            remove_worktree(root, relative)
        }
        SnapshotKind::Contents => {
            let bytes = match read_snapshot(dir, relative, kind) {
                FileState::Contents(bytes) => bytes,
                _ => return Ok(()),
            };
            write_file_atomic(&root.join(relative), &bytes, None)?;
            let _ = git_checked(root, &["reset", "-q", "HEAD", "--", relative]);
            Ok(())
        }
    }
}

fn revert_new_change(root: &Path, relative: &str) -> Result<(), String> {
    let relative = resolve_repo_path(root, relative)?;
    if in_head(root, &relative) {
        return git_checked(
            root,
            &[
                "restore",
                "--source=HEAD",
                "--staged",
                "--worktree",
                "--",
                &relative,
            ],
        );
    }
    let _ = git_checked(root, &["reset", "-q", "HEAD", "--", &relative]);
    remove_worktree(root, &relative)
}

fn in_head(root: &Path, relative: &str) -> bool {
    git_checked(root, &["cat-file", "-e", &format!("HEAD:{relative}")]).is_ok()
}

fn snapshot_file(dir: &Path, root: &Path, relative: &str) -> Result<SnapshotKind, String> {
    snapshot_file_at(&dir.join("files"), root, relative)
}

fn snapshot_after_file(dir: &Path, root: &Path, relative: &str) -> Result<SnapshotKind, String> {
    snapshot_file_at(&dir.join("after"), root, relative)
}

fn snapshot_file_at(blob_root: &Path, root: &Path, relative: &str) -> Result<SnapshotKind, String> {
    let abs = root.join(relative);
    let meta = match std::fs::symlink_metadata(&abs) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let blob = state_blob_path(blob_root, relative)?;
            if let Some(parent) = blob.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            std::fs::write(blob, []).map_err(|e| e.to_string())?;
            return Ok(SnapshotKind::Missing);
        }
        Err(error) => return Err(error.to_string()),
    };
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Ok(SnapshotKind::Skipped);
    }
    if meta.len() > MAX_TEXT_FILE_BYTES {
        return Ok(SnapshotKind::Skipped);
    }
    let bytes = std::fs::read(&abs).map_err(|e| e.to_string())?;
    let blob = state_blob_path(blob_root, relative)?;
    if let Some(parent) = blob.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&blob, bytes).map_err(|e| e.to_string())?;
    set_file_mode(&blob, file_mode(&abs))?;
    Ok(SnapshotKind::Contents)
}

#[cfg(unix)]
fn file_mode(path: &Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::symlink_metadata(path)
        .ok()
        .filter(|meta| meta.is_file() && !meta.file_type().is_symlink())
        .map(|meta| meta.permissions().mode() & 0o777)
}

#[cfg(not(unix))]
fn file_mode(_path: &Path) -> Option<u32> {
    None
}

#[cfg(unix)]
fn set_file_mode(path: &Path, mode: Option<u32>) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(mode) = mode {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn set_file_mode(_path: &Path, _mode: Option<u32>) -> Result<(), String> {
    Ok(())
}

fn snapshot_mode(blob_root: &Path, relative: &str, kind: SnapshotKind) -> Option<u32> {
    if kind != SnapshotKind::Contents {
        return None;
    }
    state_blob_path(blob_root, relative)
        .ok()
        .and_then(|path| file_mode(&path))
}

fn read_snapshot(dir: &Path, relative: &str, kind: SnapshotKind) -> FileState {
    read_snapshot_at(&dir.join("files"), relative, kind)
}

fn read_after_snapshot(dir: &Path, relative: &str, kind: SnapshotKind) -> FileState {
    read_snapshot_at(&dir.join("after"), relative, kind)
}

fn read_snapshot_at(blob_root: &Path, relative: &str, kind: SnapshotKind) -> FileState {
    match kind {
        SnapshotKind::Missing => FileState::Missing,
        SnapshotKind::Skipped => FileState::Skipped,
        SnapshotKind::Contents => match state_blob_path(blob_root, relative)
            .ok()
            .and_then(|path| std::fs::read(path).ok())
        {
            Some(bytes) => FileState::Contents(bytes),
            None => FileState::Missing,
        },
    }
}

/// Only a path that is really absent reads as Missing. Any other error,
/// such as a permission error, is Skipped so it can never become a delete.
fn read_worktree(root: &Path, relative: &str) -> FileState {
    let abs = root.join(relative);
    let read = std::fs::metadata(&abs).and_then(|meta| {
        if !meta.is_file() || meta.len() > MAX_TEXT_FILE_BYTES {
            return Ok(FileState::Skipped);
        }
        std::fs::read(&abs).map(FileState::Contents)
    });
    match read {
        Ok(state) => state,
        Err(error) if is_missing(&error) => FileState::Missing,
        Err(_) => FileState::Skipped,
    }
}

fn is_missing(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
    )
}

fn state_is_binary(state: &FileState) -> bool {
    matches!(state, FileState::Contents(bytes) if bytes.contains(&0))
}

fn state_text(state: FileState) -> String {
    match state {
        FileState::Contents(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        FileState::Missing | FileState::Skipped => String::new(),
    }
}

/// Write `bytes` to a temporary file beside `path`, set its mode, then
/// rename it over `path`, so a crash leaves either the old or the new file
/// and never a partial one. Without `mode`, an existing file keeps its own.
fn write_file_atomic(path: &Path, bytes: &[u8], mode: Option<u32>) -> Result<(), String> {
    use std::io::Write;
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(format!("Cannot write {}", path.display()));
    };
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mode = mode.or_else(|| file_mode(path));
    let temp = temp_sibling(parent, &name.to_string_lossy());
    let result = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .and_then(|mut file| {
            file.write_all(bytes)?;
            file.sync_all()
        })
        .map_err(|e| e.to_string())
        .and_then(|()| set_file_mode(&temp, mode))
        .and_then(|()| {
            std::fs::rename(&temp, path).map_err(|e| {
                if path.is_dir() {
                    format!("{} is a directory", path.display())
                } else {
                    e.to_string()
                }
            })
        });
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

fn remove_worktree(root: &Path, relative: &str) -> Result<(), String> {
    let abs = root.join(relative);
    if abs.is_file() || abs.is_symlink() {
        std::fs::remove_file(&abs).map_err(|e| e.to_string())?;
        return Ok(());
    }
    if abs.is_dir() {
        let _ = git_checked(root, &["clean", "-fd", "--", relative]);
        if abs.exists() {
            std::fs::remove_dir_all(&abs).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn state_blob_path(blob_root: &Path, relative: &str) -> Result<PathBuf, String> {
    if relative.is_empty()
        || relative.starts_with('/')
        || relative
            .split('/')
            .any(|part| part.is_empty() || part == "..")
    {
        return Err("Invalid path".into());
    }
    Ok(blob_root.join(relative))
}

fn read_manifest(dir: &Path) -> Result<Option<Manifest>, String> {
    let path = dir.join("manifest.json");
    if !path.is_file() {
        return Ok(None);
    }
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}

fn write_manifest(dir: &Path, manifest: &Manifest) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let dest = dir.join("manifest.json");
    let tmp = dir.join("manifest.json.tmp");
    let bytes = serde_json::to_vec_pretty(manifest).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(tmp, dest).map_err(|e| e.to_string())
}

fn project_root(cwd: &str) -> Result<PathBuf, String> {
    let trimmed = cwd.trim();
    if trimmed.is_empty() || trimmed == "~" {
        return Err("cwd is required".into());
    }
    let root = expand_home(trimmed);
    if !root.is_dir() {
        return Err(format!("{}: Not a directory", root.display()));
    }
    Ok(root)
}

fn same_cwd(saved: &str, cwd: &str) -> bool {
    let Ok(left) = project_root(saved) else {
        return false;
    };
    let Ok(right) = project_root(cwd) else {
        return false;
    };
    left == right
}

fn relative_to_root(root: &Path, path: &str) -> Result<String, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("Invalid path".into());
    }
    let expanded = expand_home(trimmed);
    if expanded.is_absolute() {
        let relative = expanded
            .strip_prefix(root)
            .map_err(|_| "Path is outside the project".to_string())?;
        let relative = relative.to_string_lossy().replace('\\', "/");
        return resolve_repo_path(root, &relative);
    }
    resolve_repo_path(root, trimmed)
}

fn validate_id(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(format!("Invalid {label} id"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::ErrorKind;
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

    struct Tmp(PathBuf);
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn tmp(label: &str) -> Tmp {
        loop {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "monocode-checkpoint-{label}-{}-{stamp}-{seq}",
                std::process::id()
            ));
            match std::fs::create_dir(&dir) {
                Ok(()) => return Tmp(dir),
                Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("{}", error),
            }
        }
    }

    fn git(dir: &Path, args: &[&str]) -> bool {
        Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "monocode")
            .env("GIT_AUTHOR_EMAIL", "monocode@test")
            .env("GIT_COMMITTER_NAME", "monocode")
            .env("GIT_COMMITTER_EMAIL", "monocode@test")
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    fn init_git_commit(dir: &Path, files: &[(&str, &str)]) -> bool {
        if !git(dir, &["init", "-b", "main"]) && !git(dir, &["init"]) {
            return false;
        }
        let _ = git(dir, &["config", "user.email", "monocode@test"]);
        let _ = git(dir, &["config", "user.name", "monocode"]);
        let _ = git(dir, &["config", "core.autocrlf", "false"]);
        for (name, contents) in files {
            let path = dir.join(name);
            if let Some(parent) = path.parent() {
                if std::fs::create_dir_all(parent).is_err() {
                    return false;
                }
            }
            if std::fs::write(&path, contents).is_err() {
                return false;
            }
        }
        git(dir, &["add", "."]) && git(dir, &["commit", "-m", "init"])
    }

    fn store() -> (Tmp, CheckpointStore) {
        let dir = tmp("store");
        let store = CheckpointStore::new(dir.0.clone());
        (dir, store)
    }

    fn relatives(status: &CheckpointStatus) -> Vec<&str> {
        status
            .files
            .iter()
            .map(|file| file.relative.as_str())
            .collect()
    }

    fn record(store: &CheckpointStore, id: &str, cwd: &str, paths: &[&str]) {
        let owned: Vec<String> = paths.iter().map(|path| (*path).to_string()).collect();
        store.capture(id, cwd, &owned).unwrap();
        // Most legacy tests write before calling this helper. Mark their
        // already-captured baselines as if a tool-start prepare event ran;
        // dedicated tests below exercise the real prepare/capture lifecycle.
        let root = project_root(cwd).unwrap();
        let dir = store.session_dir(id);
        let mut manifest = read_manifest(&dir).unwrap().unwrap();
        for path in paths {
            manifest
                .prepared
                .insert(relative_to_root(&root, path).unwrap());
        }
        write_manifest(&dir, &manifest).unwrap();
    }

    #[test]
    fn undo_reverts_only_session_files_and_keeps_user_dirty() {
        let repo = tmp("keep-user");
        if !init_git_commit(&repo.0, &[("user.txt", "mine\n"), ("clean.txt", "head\n")]) {
            return;
        }
        std::fs::write(repo.0.join("user.txt"), "mine-dirty\n").unwrap();
        let cwd = repo.0.to_string_lossy().into_owned();
        let (_root, store) = store();

        store.ensure("s1", &cwd, false).unwrap();

        std::fs::write(repo.0.join("user.txt"), "agent-on-user\n").unwrap();
        std::fs::write(repo.0.join("clean.txt"), "agent-on-clean\n").unwrap();
        std::fs::write(repo.0.join("new.txt"), "created\n").unwrap();
        record(&store, "s1", &cwd, &["user.txt", "clean.txt", "new.txt"]);

        let status = store.status("s1", &cwd).unwrap();
        assert_eq!(relatives(&status), vec!["clean.txt", "new.txt", "user.txt"]);

        store.undo("s1", &cwd, None).unwrap();

        assert_eq!(
            std::fs::read_to_string(repo.0.join("user.txt")).unwrap(),
            "mine-dirty\n"
        );
        assert_eq!(
            std::fs::read_to_string(repo.0.join("clean.txt")).unwrap(),
            "head\n"
        );
        assert!(!repo.0.join("new.txt").exists());
        assert!(store.status("s1", &cwd).unwrap().files.is_empty());
    }

    #[test]
    fn undo_does_not_touch_untouched_user_files() {
        let repo = tmp("untouched");
        if !init_git_commit(&repo.0, &[("keep.txt", "head\n"), ("edit.txt", "head\n")]) {
            return;
        }
        std::fs::write(repo.0.join("keep.txt"), "user\n").unwrap();
        let cwd = repo.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("s1", &cwd, false).unwrap();

        std::fs::write(repo.0.join("edit.txt"), "agent\n").unwrap();
        std::fs::write(repo.0.join("created.txt"), "new\n").unwrap();
        record(&store, "s1", &cwd, &["edit.txt", "created.txt"]);

        let status = store.status("s1", &cwd).unwrap();
        assert_eq!(relatives(&status), vec!["created.txt", "edit.txt"]);

        store.undo("s1", &cwd, None).unwrap();
        assert_eq!(
            std::fs::read_to_string(repo.0.join("keep.txt")).unwrap(),
            "user\n"
        );
        assert_eq!(
            std::fs::read_to_string(repo.0.join("edit.txt")).unwrap(),
            "head\n"
        );
        assert!(!repo.0.join("created.txt").exists());
    }

    #[test]
    fn ensure_is_idempotent_across_turns() {
        let repo = tmp("idempotent");
        if !init_git_commit(&repo.0, &[("a.txt", "head\n")]) {
            return;
        }
        std::fs::write(repo.0.join("a.txt"), "user\n").unwrap();
        let cwd = repo.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("s1", &cwd, false).unwrap();
        std::fs::write(repo.0.join("a.txt"), "agent-1\n").unwrap();
        record(&store, "s1", &cwd, &["a.txt"]);
        store.ensure("s1", &cwd, false).unwrap();
        std::fs::write(repo.0.join("b.txt"), "agent-2\n").unwrap();
        record(&store, "s1", &cwd, &["b.txt"]);

        store.undo("s1", &cwd, None).unwrap();
        assert_eq!(
            std::fs::read_to_string(repo.0.join("a.txt")).unwrap(),
            "user\n"
        );
        assert!(!repo.0.join("b.txt").exists());
    }

    #[test]
    fn keep_clears_review_and_leaves_files() {
        let repo = tmp("keep");
        if !init_git_commit(&repo.0, &[("a.txt", "head\n")]) {
            return;
        }
        let cwd = repo.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("s1", &cwd, false).unwrap();
        std::fs::write(repo.0.join("a.txt"), "agent\n").unwrap();
        std::fs::write(repo.0.join("b.txt"), "new\n").unwrap();
        record(&store, "s1", &cwd, &["a.txt", "b.txt"]);
        assert!(!store.status("s1", &cwd).unwrap().files.is_empty());

        store.keep("s1", &cwd, None).unwrap();
        assert!(store.status("s1", &cwd).unwrap().files.is_empty());
        assert_eq!(
            std::fs::read_to_string(repo.0.join("a.txt")).unwrap(),
            "agent\n"
        );
        assert_eq!(
            std::fs::read_to_string(repo.0.join("b.txt")).unwrap(),
            "new\n"
        );
    }

    #[test]
    fn keep_one_file_then_undo_the_rest() {
        let repo = tmp("keep-one");
        if !init_git_commit(&repo.0, &[("a.txt", "head-a\n"), ("b.txt", "head-b\n")]) {
            return;
        }
        let cwd = repo.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("s1", &cwd, false).unwrap();
        std::fs::write(repo.0.join("a.txt"), "agent-a\n").unwrap();
        std::fs::write(repo.0.join("b.txt"), "agent-b\n").unwrap();
        record(&store, "s1", &cwd, &["a.txt", "b.txt"]);

        store.keep("s1", &cwd, Some("a.txt")).unwrap();
        let status = store.status("s1", &cwd).unwrap();
        assert_eq!(relatives(&status), vec!["b.txt"]);

        store.undo("s1", &cwd, None).unwrap();
        assert_eq!(
            std::fs::read_to_string(repo.0.join("a.txt")).unwrap(),
            "agent-a\n"
        );
        assert_eq!(
            std::fs::read_to_string(repo.0.join("b.txt")).unwrap(),
            "head-b\n"
        );
    }

    #[test]
    fn ensure_baselines_other_session_dirty_files() {
        let repo = tmp("ensure-baseline");
        if !init_git_commit(&repo.0, &[("plan.md", "old\n")]) {
            return;
        }
        let cwd = repo.0.to_string_lossy().into_owned();
        let (_root, store) = store();

        store.ensure("s1", &cwd, false).unwrap();
        std::fs::write(repo.0.join("plan.md"), "session-one\n").unwrap();
        record(&store, "s1", &cwd, &["plan.md"]);

        store.ensure("s2", &cwd, false).unwrap();
        assert!(store.status("s2", &cwd).unwrap().files.is_empty());
    }

    #[test]
    fn other_session_edits_do_not_appear_in_review() {
        let repo = tmp("two-sessions");
        if !init_git_commit(&repo.0, &[("plan.md", "old\n"), ("readme.md", "old\n")]) {
            return;
        }
        let cwd = repo.0.to_string_lossy().into_owned();
        let (_root, store) = store();

        store.ensure("s1", &cwd, false).unwrap();
        std::fs::write(repo.0.join("plan.md"), "session-one\n").unwrap();
        record(&store, "s1", &cwd, &["plan.md"]);
        assert_eq!(
            relatives(&store.status("s1", &cwd).unwrap()),
            vec!["plan.md"]
        );

        store.ensure("s2", &cwd, false).unwrap();
        std::fs::write(repo.0.join("readme.md"), "session-two\n").unwrap();
        record(&store, "s2", &cwd, &["readme.md"]);

        assert_eq!(
            relatives(&store.status("s1", &cwd).unwrap()),
            vec!["plan.md"]
        );
        assert_eq!(
            relatives(&store.status("s2", &cwd).unwrap()),
            vec!["readme.md"]
        );

        store.undo("s1", &cwd, None).unwrap();
        assert_eq!(
            std::fs::read_to_string(repo.0.join("plan.md")).unwrap(),
            "old\n"
        );
        assert_eq!(
            std::fs::read_to_string(repo.0.join("readme.md")).unwrap(),
            "session-two\n"
        );
    }

    #[test]
    fn read_only_session_has_no_changes_when_another_session_edits() {
        let repo = tmp("read-only-session");
        if !init_git_commit(&repo.0, &[("app.ts", "head\n")]) {
            return;
        }
        let cwd = repo.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("writer", &cwd, false).unwrap();
        store.ensure("reader", &cwd, false).unwrap();

        store.prepare("writer", &cwd, &["app.ts".into()]).unwrap();
        std::fs::write(repo.0.join("app.ts"), "writer\n").unwrap();
        store.capture("writer", &cwd, &["app.ts".into()]).unwrap();

        assert_eq!(
            relatives(&store.status("writer", &cwd).unwrap()),
            vec!["app.ts"]
        );
        assert!(store.status("reader", &cwd).unwrap().files.is_empty());
    }

    #[test]
    fn status_counts_only_the_session_delta_from_its_pre_edit_snapshot() {
        let repo = tmp("session-counts");
        if !init_git_commit(&repo.0, &[("app.ts", "head\n")]) {
            return;
        }
        std::fs::write(repo.0.join("app.ts"), "user-one\nuser-two\n").unwrap();
        let cwd = repo.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("s1", &cwd, false).unwrap();
        store.prepare("s1", &cwd, &["app.ts".into()]).unwrap();
        std::fs::write(repo.0.join("app.ts"), "user-one\nuser-two\nagent\n").unwrap();
        store.capture("s1", &cwd, &["app.ts".into()]).unwrap();

        let status = store.status("s1", &cwd).unwrap();
        assert_eq!(status.files.len(), 1);
        assert_eq!(status.files[0].additions, 1);
        assert_eq!(status.files[0].deletions, 0);
        assert!(status.files[0].exact);
        assert!(status.files[0].undoable);
        assert_eq!(status.files[0].path, path_to_js(&repo.0.join("app.ts")));

        let diff = store.file_diff("s1", &cwd, "app.ts").unwrap();
        assert_eq!(diff.original, "user-one\nuser-two\n");
        assert_eq!(diff.current, "user-one\nuser-two\nagent\n");
        assert_eq!(diff.path, path_to_js(&repo.0.join("app.ts")));

        // Review remains the captured session result, not a later shared
        // working-tree state.
        std::fs::write(repo.0.join("app.ts"), "user-one\nuser-two\nagent\nother\n").unwrap();
        let diff = store.file_diff("s1", &cwd, "app.ts").unwrap();
        assert_eq!(diff.current, "user-one\nuser-two\nagent\n");
    }

    #[test]
    fn shared_file_keeps_session_scoped_review_but_disables_unsafe_undo() {
        let repo = tmp("shared-file");
        if !init_git_commit(&repo.0, &[("app.ts", "head\n")]) {
            return;
        }
        let cwd = repo.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("s1", &cwd, false).unwrap();
        store.ensure("s2", &cwd, false).unwrap();

        store.prepare("s1", &cwd, &["app.ts".into()]).unwrap();
        std::fs::write(repo.0.join("app.ts"), "session-one\n").unwrap();
        store.capture("s1", &cwd, &["app.ts".into()]).unwrap();

        store.prepare("s2", &cwd, &["app.ts".into()]).unwrap();
        std::fs::write(repo.0.join("app.ts"), "session-one\nsession-two\n").unwrap();
        store.capture("s2", &cwd, &["app.ts".into()]).unwrap();

        let s1 = store.status("s1", &cwd).unwrap();
        let s2 = store.status("s2", &cwd).unwrap();
        assert!(s1.files[0].exact);
        assert!(s2.files[0].exact);
        assert_eq!((s1.files[0].additions, s1.files[0].deletions), (1, 1));
        assert_eq!((s2.files[0].additions, s2.files[0].deletions), (1, 0));
        assert!(!s1.files[0].undoable);
        assert!(!s2.files[0].undoable);
        let s1_diff = store.file_diff("s1", &cwd, "app.ts").unwrap();
        assert_eq!(s1_diff.original, "head\n");
        assert_eq!(s1_diff.current, "session-one\n");
        let s2_diff = store.file_diff("s2", &cwd, "app.ts").unwrap();
        assert_eq!(s2_diff.original, "session-one\n");
        assert_eq!(s2_diff.current, "session-one\nsession-two\n");
        assert!(store.undo("s1", &cwd, None).is_err());
        assert_eq!(
            std::fs::read_to_string(repo.0.join("app.ts")).unwrap(),
            "session-one\nsession-two\n"
        );

        // Accepting s1 releases its ownership without touching the file. The
        // second session can then safely undo back to the contents it started
        // from, preserving s1's accepted line.
        store.keep("s1", &cwd, None).unwrap();
        assert!(store.status("s2", &cwd).unwrap().files[0].undoable);
        store.undo("s2", &cwd, None).unwrap();
        assert_eq!(
            std::fs::read_to_string(repo.0.join("app.ts")).unwrap(),
            "session-one\n"
        );
    }

    #[test]
    fn undo_refuses_a_file_changed_after_the_session_edit() {
        let repo = tmp("changed-after");
        if !init_git_commit(&repo.0, &[("app.ts", "head\n")]) {
            return;
        }
        let cwd = repo.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("s1", &cwd, false).unwrap();
        store.prepare("s1", &cwd, &["app.ts".into()]).unwrap();
        std::fs::write(repo.0.join("app.ts"), "agent\n").unwrap();
        store.capture("s1", &cwd, &["app.ts".into()]).unwrap();

        std::fs::write(repo.0.join("app.ts"), "agent\nother\n").unwrap();

        assert!(!store.status("s1", &cwd).unwrap().files[0].undoable);
        assert!(store.undo("s1", &cwd, None).is_err());
        assert_eq!(
            std::fs::read_to_string(repo.0.join("app.ts")).unwrap(),
            "agent\nother\n"
        );
    }

    #[test]
    fn capture_missing_path_lets_non_git_undo_delete() {
        let project = tmp("nongit");
        let cwd = project.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("s1", &cwd, false).unwrap();
        store
            .prepare(
                "s1",
                &cwd,
                &[project.0.join("made.txt").to_string_lossy().into_owned()],
            )
            .unwrap();
        std::fs::write(project.0.join("made.txt"), "hello\n").unwrap();
        store
            .capture(
                "s1",
                &cwd,
                &[project.0.join("made.txt").to_string_lossy().into_owned()],
            )
            .unwrap();
        assert_eq!(
            relatives(&store.status("s1", &cwd).unwrap()),
            vec!["made.txt"]
        );
        store.undo("s1", &cwd, None).unwrap();
        assert!(!project.0.join("made.txt").exists());
    }

    #[test]
    fn late_capture_is_not_attributed_to_the_session() {
        let repo = tmp("late-capture");
        if !init_git_commit(&repo.0, &[("a.txt", "head\n")]) {
            return;
        }
        let cwd = repo.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("s1", &cwd, false).unwrap();
        std::fs::write(repo.0.join("a.txt"), "agent\n").unwrap();
        store.capture("s1", &cwd, &["a.txt".into()]).unwrap();
        assert!(store.status("s1", &cwd).unwrap().files.is_empty());
        store.undo("s1", &cwd, None).unwrap();
        assert_eq!(
            std::fs::read_to_string(repo.0.join("a.txt")).unwrap(),
            "agent\n"
        );

        // A later structured edit in that same session replaces the
        // untrusted completion-only claim with a real boundary.
        store.ensure("s1", &cwd, false).unwrap();
        store.capture("s1", &cwd, &["a.txt".into()]).unwrap();
        store.prepare("s1", &cwd, &["a.txt".into()]).unwrap();
        std::fs::write(repo.0.join("a.txt"), "same-session-valid\n").unwrap();
        store.capture("s1", &cwd, &["a.txt".into()]).unwrap();
        assert!(store.status("s1", &cwd).unwrap().files[0].undoable);
        store.undo("s1", &cwd, None).unwrap();
        assert_eq!(
            std::fs::read_to_string(repo.0.join("a.txt")).unwrap(),
            "agent\n"
        );

        // An old/unprepared claim is not ownership and must not block a later
        // session that recorded a trustworthy before/after pair.
        store.ensure("s2", &cwd, false).unwrap();
        store.prepare("s2", &cwd, &["a.txt".into()]).unwrap();
        std::fs::write(repo.0.join("a.txt"), "second-session\n").unwrap();
        store.capture("s2", &cwd, &["a.txt".into()]).unwrap();
        assert!(store.status("s2", &cwd).unwrap().files[0].undoable);
        store.undo("s2", &cwd, None).unwrap();
        assert_eq!(
            std::fs::read_to_string(repo.0.join("a.txt")).unwrap(),
            "agent\n"
        );
    }

    #[test]
    fn committed_session_changes_leave_review() {
        let repo = tmp("committed");
        if !init_git_commit(&repo.0, &[("edit.txt", "head\n"), ("delete.txt", "head\n")]) {
            return;
        }
        let cwd = repo.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("s1", &cwd, false).unwrap();

        std::fs::write(repo.0.join("edit.txt"), "agent\n").unwrap();
        std::fs::write(repo.0.join("created.txt"), "new\n").unwrap();
        std::fs::remove_file(repo.0.join("delete.txt")).unwrap();
        record(
            &store,
            "s1",
            &cwd,
            &["edit.txt", "created.txt", "delete.txt"],
        );
        assert_eq!(
            relatives(&store.status("s1", &cwd).unwrap()),
            vec!["created.txt", "delete.txt", "edit.txt"]
        );

        assert!(git(&repo.0, &["add", "-A"]));
        assert!(git(&repo.0, &["commit", "-m", "agent changes"]));
        assert!(store.status("s1", &cwd).unwrap().files.is_empty());
    }

    #[test]
    fn deleting_untracked_baseline_still_needs_review() {
        let repo = tmp("delete-untracked");
        if !init_git_commit(&repo.0, &[("tracked.txt", "head\n")]) {
            return;
        }
        std::fs::write(repo.0.join("loose.txt"), "user\n").unwrap();
        let cwd = repo.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("s1", &cwd, false).unwrap();

        std::fs::remove_file(repo.0.join("loose.txt")).unwrap();
        record(&store, "s1", &cwd, &["loose.txt"]);
        assert_eq!(
            relatives(&store.status("s1", &cwd).unwrap()),
            vec!["loose.txt"]
        );
    }

    #[test]
    fn session_stats_match_git_not_edit_churn() {
        let repo = tmp("stats-churn");
        if !init_git_commit(&repo.0, &[("a.txt", "head\n")]) {
            return;
        }
        let cwd = repo.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("s1", &cwd, false).unwrap();

        std::fs::write(repo.0.join("a.txt"), "one\ntwo\nthree\nfour\n").unwrap();
        record(&store, "s1", &cwd, &["a.txt"]);
        std::fs::write(repo.0.join("a.txt"), "head\nworld\n").unwrap();
        record(&store, "s1", &cwd, &["a.txt"]);

        let stats = store.stats_for_sessions(&cwd, &["s1".into()]).unwrap();
        let s1 = stats.get("s1").expect("s1 stats");
        assert_eq!(s1.files, 1);
        assert_eq!(s1.additions, 1);
        assert_eq!(s1.deletions, 0);
    }

    #[test]
    fn session_stats_are_scoped_to_touched_files() {
        let repo = tmp("stats-scoped");
        if !init_git_commit(&repo.0, &[("a.txt", "a\n"), ("b.txt", "b\n")]) {
            return;
        }
        std::fs::write(repo.0.join("b.txt"), "user\n").unwrap();
        let cwd = repo.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("s1", &cwd, false).unwrap();

        std::fs::write(repo.0.join("a.txt"), "a\nA\n").unwrap();
        record(&store, "s1", &cwd, &["a.txt"]);

        let stats = store.stats_for_sessions(&cwd, &["s1".into()]).unwrap();
        let s1 = stats.get("s1").expect("s1 stats");
        assert_eq!(s1.files, 1);
        assert_eq!(s1.additions, 1);
        assert_eq!(s1.deletions, 0);
        assert_eq!(relatives(&store.status("s1", &cwd).unwrap()), vec!["a.txt"]);
    }

    #[test]
    fn isolated_worker_delta_applies_idempotently_to_matching_baseline() {
        let source = tmp("apply-source");
        let target = tmp("apply-target");
        if !init_git_commit(&source.0, &[("a.txt", "head\n")]) {
            return;
        }
        let source_path = source.0.to_string_lossy().into_owned();
        let target_path = target.0.to_string_lossy().into_owned();
        if !git(&source.0, &["clone", &source_path, &target_path]) {
            return;
        }
        std::fs::write(source.0.join("a.txt"), "user baseline\n").unwrap();
        std::fs::write(target.0.join("a.txt"), "user baseline\n").unwrap();
        let from = source.0.to_string_lossy().into_owned();
        let to = target.0.to_string_lossy().into_owned();
        let (_root, store) = store();

        store.ensure("worker", &from, false).unwrap();
        assert!(store.cleanup_safe("worker", &from).unwrap());
        store.prepare("worker", &from, &["a.txt".into()]).unwrap();
        std::fs::write(source.0.join("a.txt"), "worker result\n").unwrap();
        store.capture("worker", &from, &["a.txt".into()]).unwrap();
        assert!(!store.cleanup_safe("worker", &from).unwrap());

        let applied = store.apply("worker", &from, &to, None).unwrap();
        assert_eq!(applied.files, ["a.txt"]);
        assert_eq!(applied.already_applied, 0);
        assert_eq!(
            std::fs::read_to_string(target.0.join("a.txt")).unwrap(),
            "worker result\n"
        );
        let retried = store.apply("worker", &from, &to, None).unwrap();
        assert_eq!(retried.already_applied, 1);
    }

    #[test]
    fn isolated_worker_integration_keeps_both_sides_on_conflict_or_unknown_edit() {
        let source = tmp("apply-conflict-source");
        let target = tmp("apply-conflict-target");
        if !init_git_commit(&source.0, &[("a.txt", "head\n")]) {
            return;
        }
        let source_path = source.0.to_string_lossy().into_owned();
        let target_path = target.0.to_string_lossy().into_owned();
        if !git(&source.0, &["clone", &source_path, &target_path]) {
            return;
        }
        let from = source.0.to_string_lossy().into_owned();
        let to = target.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("worker", &from, false).unwrap();
        store.prepare("worker", &from, &["a.txt".into()]).unwrap();
        std::fs::write(source.0.join("a.txt"), "worker\n").unwrap();
        store.capture("worker", &from, &["a.txt".into()]).unwrap();
        std::fs::write(target.0.join("a.txt"), "lead changed\n").unwrap();

        let conflict = store.apply("worker", &from, &to, None).unwrap_err();
        assert!(conflict.contains("lead checkout changed"));
        assert_eq!(
            std::fs::read_to_string(source.0.join("a.txt")).unwrap(),
            "worker\n"
        );
        assert_eq!(
            std::fs::read_to_string(target.0.join("a.txt")).unwrap(),
            "lead changed\n"
        );

        std::fs::write(source.0.join("unreported.txt"), "unknown\n").unwrap();
        assert!(!store.cleanup_safe("worker", &from).unwrap());
        assert!(store
            .apply("worker", &from, &to, None)
            .unwrap_err()
            .contains("not captured"));
    }

    #[test]
    fn worker_new_and_modified_files_apply_to_lead_worktree() {
        let lead = tmp("worker-lead");
        if !init_git_commit(&lead.0, &[("tracked.txt", "head\n")]) {
            return;
        }
        let worker = lead.0.join("worker-tree");
        let worker_path = worker.to_string_lossy().into_owned();
        if !git(&lead.0, &["worktree", "add", "-b", "worker", &worker_path]) {
            return;
        }
        let from = worker_path;
        let to = lead.0.to_string_lossy().into_owned();
        let (_root, store) = store();

        // Without ensure, prepare and capture record nothing and apply names
        // the worktree so the user can recover by hand.
        store
            .prepare("worker", &from, &["tracked.txt".into()])
            .unwrap();
        store
            .capture("worker", &from, &["tracked.txt".into()])
            .unwrap();
        let missing = store.apply("worker", &from, &to, None).unwrap_err();
        assert!(missing.contains("no change checkpoint"));
        assert!(missing.contains(&from));

        store.ensure("worker", &from, false).unwrap();
        let edited = vec!["tracked.txt".to_string(), "src/new.txt".to_string()];
        store.prepare("worker", &from, &edited).unwrap();
        std::fs::write(worker.join("tracked.txt"), "worker\n").unwrap();
        std::fs::create_dir_all(worker.join("src")).unwrap();
        std::fs::write(worker.join("src/new.txt"), "created\n").unwrap();
        store.capture("worker", &from, &edited).unwrap();

        let applied = store.apply("worker", &from, &to, None).unwrap();
        assert_eq!(applied.files, ["src/new.txt", "tracked.txt"]);
        assert_eq!(
            std::fs::read_to_string(lead.0.join("tracked.txt")).unwrap(),
            "worker\n"
        );
        assert_eq!(
            std::fs::read_to_string(lead.0.join("src/new.txt")).unwrap(),
            "created\n"
        );
    }

    /// A lead repo plus a worker worktree at the same HEAD. Returns the
    /// worker path, or None when git is unavailable.
    fn lead_and_worker(lead: &Path, files: &[(&str, &str)]) -> Option<PathBuf> {
        if !init_git_commit(lead, files) {
            return None;
        }
        let worker = lead.join("worker-tree");
        let worker_path = worker.to_string_lossy().into_owned();
        if !git(lead, &["worktree", "add", "-b", "worker", &worker_path]) {
            return None;
        }
        Some(worker)
    }

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap()
    }

    #[test]
    fn isolated_worker_shell_edits_apply_without_tool_events() {
        let lead = tmp("isolated-shell");
        let Some(worker) = lead_and_worker(
            &lead.0,
            &[
                ("tracked.txt", "head\n"),
                ("gone.txt", "head\n"),
                ("keep.txt", "keep\n"),
            ],
        ) else {
            return;
        };
        let from = worker.to_string_lossy().into_owned();
        let to = lead.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("worker", &from, true).unwrap();

        std::fs::write(worker.join("tracked.txt"), "worker\n").unwrap();
        std::fs::create_dir_all(worker.join("fresh/nested")).unwrap();
        std::fs::write(worker.join("fresh/nested/new.txt"), "created\n").unwrap();
        std::fs::remove_file(worker.join("gone.txt")).unwrap();

        let applied = store.apply("worker", &from, &to, None).unwrap();
        assert_eq!(
            applied.files,
            ["fresh/nested/new.txt", "gone.txt", "tracked.txt"]
        );
        assert_eq!(applied.already_applied, 0);
        assert_eq!(read(&lead.0.join("tracked.txt")), "worker\n");
        assert_eq!(read(&lead.0.join("fresh/nested/new.txt")), "created\n");
        assert!(!lead.0.join("gone.txt").exists());
        assert_eq!(read(&lead.0.join("keep.txt")), "keep\n");

        let retried = store.apply("worker", &from, &to, None).unwrap();
        assert_eq!(retried.files, applied.files);
        assert_eq!(retried.already_applied, 3);
    }

    #[test]
    fn isolated_worker_capture_without_prepare_applies() {
        let lead = tmp("isolated-fx");
        let Some(worker) = lead_and_worker(&lead.0, &[("src/app.ts", "head\n")]) else {
            return;
        };
        let from = worker.to_string_lossy().into_owned();
        let to = lead.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("worker", &from, true).unwrap();

        std::fs::write(worker.join("src/app.ts"), "fx edit\n").unwrap();
        store
            .capture("worker", &from, &["src/app.ts".into()])
            .unwrap();

        let applied = store.apply("worker", &from, &to, None).unwrap();
        assert_eq!(applied.files, ["src/app.ts"]);
        assert_eq!(read(&lead.0.join("src/app.ts")), "fx edit\n");
    }

    #[test]
    fn isolated_worker_respects_the_seed_baseline() {
        let lead = tmp("isolated-seed");
        if !init_git_commit(
            &lead.0,
            &[
                ("seeded.txt", "head\n"),
                ("mixed.txt", "head\n"),
                ("work.txt", "head\n"),
            ],
        ) {
            return;
        }
        std::fs::write(lead.0.join("seeded.txt"), "lead dirty\n").unwrap();
        std::fs::write(lead.0.join("mixed.txt"), "lead dirty\n").unwrap();
        std::fs::write(lead.0.join("loose.txt"), "lead untracked\n").unwrap();
        let worker = lead.0.join("worker-tree");
        let worker_path = worker.to_string_lossy().into_owned();
        if !git(&lead.0, &["worktree", "add", "-b", "worker", &worker_path]) {
            return;
        }
        // Simulate seeding the worktree with the lead's uncommitted changes.
        std::fs::write(worker.join("seeded.txt"), "lead dirty\n").unwrap();
        std::fs::write(worker.join("mixed.txt"), "lead dirty\n").unwrap();
        std::fs::write(worker.join("loose.txt"), "lead untracked\n").unwrap();
        let from = worker_path;
        let to = lead.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("worker", &from, true).unwrap();
        assert!(store.cleanup_safe("worker", &from).unwrap());

        std::fs::write(worker.join("work.txt"), "worker\n").unwrap();
        // A shell edit to a seeded file followed by a structured edit: the
        // prepare snapshot is not the seed, but the delta must still be.
        std::fs::write(worker.join("mixed.txt"), "lead dirty\nshell\n").unwrap();
        store
            .prepare("worker", &from, &["mixed.txt".into()])
            .unwrap();
        std::fs::write(worker.join("mixed.txt"), "lead dirty\nshell\ntool\n").unwrap();
        store
            .capture("worker", &from, &["mixed.txt".into()])
            .unwrap();

        let applied = store.apply("worker", &from, &to, None).unwrap();
        assert_eq!(applied.files, ["mixed.txt", "work.txt"]);
        assert_eq!(read(&lead.0.join("seeded.txt")), "lead dirty\n");
        assert_eq!(read(&lead.0.join("loose.txt")), "lead untracked\n");
        assert_eq!(read(&lead.0.join("mixed.txt")), "lead dirty\nshell\ntool\n");
        assert_eq!(read(&lead.0.join("work.txt")), "worker\n");
    }

    #[test]
    fn isolated_worker_ensure_refuses_an_incomplete_seed() {
        let repo = tmp("isolated-too-many");
        if !init_git_commit(&repo.0, &[("a.txt", "head\n")]) {
            return;
        }
        for index in 0..=MAX_SNAPSHOT_FILES {
            std::fs::write(repo.0.join(format!("f{index}.txt")), "x\n").unwrap();
        }
        let cwd = repo.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        let error = store.ensure("worker", &cwd, true).unwrap_err();
        assert!(error.contains("too many uncommitted files"));
        assert!(store.ensure("lead", &cwd, false).is_ok());
    }

    #[test]
    fn isolated_worker_apply_leaves_out_of_scope_files_in_the_worktree() {
        let lead = tmp("isolated-scope");
        let Some(worker) = lead_and_worker(
            &lead.0,
            &[("src/app.ts", "head\n"), ("src2/x.ts", "head\n")],
        ) else {
            return;
        };
        let from = worker.to_string_lossy().into_owned();
        let to = lead.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("worker", &from, true).unwrap();

        std::fs::write(worker.join("src/app.ts"), "worker\n").unwrap();
        std::fs::write(worker.join("src2/x.ts"), "worker\n").unwrap();
        std::fs::write(worker.join("notes.md"), "worker\n").unwrap();

        // `src` must not admit `src2/x.ts`.
        let src = worker.join("src").to_string_lossy().into_owned();
        let applied = store
            .apply("worker", &from, &to, Some(&[src.clone()]))
            .unwrap();
        assert_eq!(applied.files, ["src/app.ts"]);
        assert_eq!(applied.skipped, ["notes.md", "src2/x.ts"]);
        assert_eq!(read(&lead.0.join("src/app.ts")), "worker\n");
        assert_eq!(read(&lead.0.join("src2/x.ts")), "head\n");
        assert!(!lead.0.join("notes.md").exists());
        assert_eq!(read(&worker.join("notes.md")), "worker\n");
        assert!(!store.cleanup_safe("worker", &from).unwrap());

        // A retry is idempotent and still reports the skipped files.
        let retried = store
            .apply("worker", &from, &to, Some(&[src.clone()]))
            .unwrap();
        assert_eq!(retried.already_applied, 1);
        assert_eq!(retried.skipped, ["notes.md", "src2/x.ts"]);

        // A scope may be a file that does not exist in the lead or worker
        // yet, and may be spelled through a symlinked temp directory.
        std::fs::remove_file(worker.join("notes.md")).unwrap();
        std::fs::write(worker.join("src2/x.ts"), "head\n").unwrap();
        std::fs::write(worker.join("docs.md"), "worker\n").unwrap();
        let docs = std::fs::canonicalize(&worker)
            .unwrap()
            .join("docs.md")
            .to_string_lossy()
            .into_owned();
        let applied = store
            .apply("worker", &from, &to, Some(&[src, docs]))
            .unwrap();
        assert_eq!(applied.files, ["docs.md", "src/app.ts"]);
        assert!(applied.skipped.is_empty());
        assert_eq!(read(&lead.0.join("docs.md")), "worker\n");
    }

    #[cfg(unix)]
    #[test]
    fn isolated_worker_delete_ignores_permission_bits_git_does_not_track() {
        use std::os::unix::fs::PermissionsExt;
        let lead = tmp("isolated-umask");
        let Some(worker) = lead_and_worker(&lead.0, &[("gone.txt", "head\n")]) else {
            return;
        };
        // The lead's copy has group write, as under umask 002.
        std::fs::set_permissions(
            lead.0.join("gone.txt"),
            std::fs::Permissions::from_mode(0o664),
        )
        .unwrap();
        let from = worker.to_string_lossy().into_owned();
        let to = lead.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("worker", &from, true).unwrap();
        std::fs::remove_file(worker.join("gone.txt")).unwrap();

        let applied = store.apply("worker", &from, &to, None).unwrap();
        assert_eq!(applied.files, ["gone.txt"]);
        assert!(!lead.0.join("gone.txt").exists());

        // The executable bit is tracked, so flipping it is still a conflict.
        let lead2 = tmp("isolated-umask-exec");
        let Some(worker2) = lead_and_worker(&lead2.0, &[("gone.txt", "head\n")]) else {
            return;
        };
        std::fs::set_permissions(
            lead2.0.join("gone.txt"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        let from2 = worker2.to_string_lossy().into_owned();
        let to2 = lead2.0.to_string_lossy().into_owned();
        store.ensure("worker2", &from2, true).unwrap();
        std::fs::remove_file(worker2.join("gone.txt")).unwrap();
        assert!(store
            .apply("worker2", &from2, &to2, None)
            .unwrap_err()
            .contains("lead checkout changed"));
    }

    #[test]
    fn isolated_worker_cleanup_safe_tracks_shell_edits() {
        let lead = tmp("isolated-cleanup");
        let Some(worker) = lead_and_worker(&lead.0, &[("a.txt", "head\n")]) else {
            return;
        };
        let from = worker.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("worker", &from, true).unwrap();
        assert!(store.cleanup_safe("worker", &from).unwrap());

        std::fs::write(worker.join("a.txt"), "shell\n").unwrap();
        assert!(!store.cleanup_safe("worker", &from).unwrap());
        std::fs::write(worker.join("a.txt"), "head\n").unwrap();
        assert!(store.cleanup_safe("worker", &from).unwrap());
        std::fs::write(worker.join("new.txt"), "shell\n").unwrap();
        assert!(!store.cleanup_safe("worker", &from).unwrap());
    }

    #[test]
    fn isolated_worker_applies_names_with_spaces_and_arrows_literally() {
        let lead = tmp("isolated-literal");
        let Some(worker) = lead_and_worker(
            &lead.0,
            &[("b.txt", "lead b\n"), ("notes.txt", "lead notes\n")],
        ) else {
            return;
        };
        let from = worker.to_string_lossy().into_owned();
        let to = lead.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("worker", &from, true).unwrap();

        std::fs::write(worker.join("a => b.txt"), "arrow\n").unwrap();
        std::fs::write(worker.join("notes.txt "), "trailing space\n").unwrap();

        let applied = store.apply("worker", &from, &to, None).unwrap();
        assert_eq!(applied.files, ["a => b.txt", "notes.txt "]);
        assert_eq!(read(&lead.0.join("a => b.txt")), "arrow\n");
        assert_eq!(read(&lead.0.join("notes.txt ")), "trailing space\n");
        assert_eq!(read(&lead.0.join("b.txt")), "lead b\n");
        assert_eq!(read(&lead.0.join("notes.txt")), "lead notes\n");
    }

    #[test]
    fn isolated_worker_deletes_a_tracked_arrow_name_literally() {
        let lead = tmp("isolated-literal-delete");
        let Some(worker) = lead_and_worker(&lead.0, &[("a => b.txt", "arrow\n"), ("b.txt", "b\n")])
        else {
            return;
        };
        let from = worker.to_string_lossy().into_owned();
        let to = lead.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("worker", &from, true).unwrap();
        std::fs::remove_file(worker.join("a => b.txt")).unwrap();

        let applied = store.apply("worker", &from, &to, None).unwrap();
        assert_eq!(applied.files, ["a => b.txt"]);
        assert!(!lead.0.join("a => b.txt").exists());
        assert_eq!(read(&lead.0.join("b.txt")), "b\n");
    }

    #[cfg(unix)]
    #[test]
    fn isolated_worker_scope_through_a_symlink_matches_nothing() {
        let lead = tmp("isolated-scope-symlink");
        let Some(worker) = lead_and_worker(
            &lead.0,
            &[
                ("src/app.ts", "head\n"),
                ("Makefile", "head\n"),
                (".github/workflows/ci.yml", "head\n"),
            ],
        ) else {
            return;
        };
        // Linked worktrees share the main repository's info/exclude.
        std::fs::write(lead.0.join(".git/info/exclude"), "src/feature\n").unwrap();
        let from = worker.to_string_lossy().into_owned();
        let to = lead.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("worker", &from, true).unwrap();

        std::os::unix::fs::symlink("..", worker.join("src/feature")).unwrap();
        std::fs::write(worker.join("Makefile"), "worker\n").unwrap();
        std::fs::write(worker.join(".github/workflows/ci.yml"), "worker\n").unwrap();

        let scopes = [
            worker.join("src/feature").to_string_lossy().into_owned(),
            std::fs::canonicalize(&worker)
                .unwrap()
                .join("src/feature/")
                .to_string_lossy()
                .into_owned(),
        ];
        let applied = store.apply("worker", &from, &to, Some(&scopes)).unwrap();
        assert!(applied.files.is_empty());
        assert_eq!(applied.skipped, [".github/workflows/ci.yml", "Makefile"]);
        assert_eq!(read(&lead.0.join("Makefile")), "head\n");
        assert_eq!(read(&lead.0.join(".github/workflows/ci.yml")), "head\n");

        // `.` still means the whole checkout.
        let all = [".".to_string()];
        let applied = store.apply("worker", &from, &to, Some(&all)).unwrap();
        assert_eq!(applied.files, [".github/workflows/ci.yml", "Makefile"]);
    }

    #[cfg(unix)]
    #[test]
    fn isolated_worker_unreadable_file_is_not_a_delete() {
        use std::os::unix::fs::PermissionsExt;
        struct Restore(PathBuf);
        impl Drop for Restore {
            fn drop(&mut self) {
                let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o644));
            }
        }

        let lead = tmp("isolated-unreadable");
        let Some(worker) = lead_and_worker(&lead.0, &[("a.txt", "head\n")]) else {
            return;
        };
        // Seed a dirty copy so the path is a candidate whatever git reports.
        std::fs::write(worker.join("a.txt"), "dirty\n").unwrap();
        std::fs::write(lead.0.join("a.txt"), "dirty\n").unwrap();
        let from = worker.to_string_lossy().into_owned();
        let to = lead.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("worker", &from, true).unwrap();

        let path = worker.join("a.txt");
        let _restore = Restore(path.clone());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read(&path).is_ok() {
            // Running as root, where permissions do not stop reads.
            return;
        }
        assert_eq!(read_worktree(&worker, "a.txt"), FileState::Skipped);
        let error = store.apply("worker", &from, &to, None).unwrap_err();
        assert!(error.contains("cannot be checkpointed"), "{error}");
        assert!(!store.cleanup_safe("worker", &from).unwrap());
        assert_eq!(read(&lead.0.join("a.txt")), "dirty\n");
    }

    #[test]
    fn isolated_worker_applies_ignored_files_edited_by_tools() {
        let lead = tmp("isolated-ignored");
        let Some(worker) = lead_and_worker(
            &lead.0,
            &[(".gitignore", ".env\n*.log\n"), ("a.txt", "a\n")],
        ) else {
            return;
        };
        let from = worker.to_string_lossy().into_owned();
        let to = lead.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("worker", &from, true).unwrap();

        store.prepare("worker", &from, &[".env".into()]).unwrap();
        std::fs::write(worker.join(".env"), "TOKEN=1\n").unwrap();
        store.capture("worker", &from, &[".env".into()]).unwrap();
        // Ignored files made only by shell commands are reported, not written.
        std::fs::write(worker.join("debug.log"), "noise\n").unwrap();
        assert!(!store.cleanup_safe("worker", &from).unwrap());

        let applied = store.apply("worker", &from, &to, None).unwrap();
        assert_eq!(applied.files, [".env"]);
        assert_eq!(applied.ignored, ["debug.log"]);
        assert_eq!(read(&lead.0.join(".env")), "TOKEN=1\n");
        assert!(!lead.0.join("debug.log").exists());
    }

    #[test]
    fn isolated_worker_untouched_large_seed_file_does_not_block_apply() {
        let lead = tmp("isolated-large-seed");
        let Some(worker) = lead_and_worker(&lead.0, &[("a.txt", "head\n")]) else {
            return;
        };
        // Sparse files keep this fast.
        let big = worker.join("big.bin");
        std::fs::File::create(&big)
            .unwrap()
            .set_len(MAX_TEXT_FILE_BYTES + 1)
            .unwrap();
        let from = worker.to_string_lossy().into_owned();
        let to = lead.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("worker", &from, true).unwrap();
        let manifest = read_manifest(&store.session_dir("worker"))
            .unwrap()
            .unwrap();
        assert!(manifest.seed_stat.contains_key("big.bin"));

        std::fs::write(worker.join("a.txt"), "worker\n").unwrap();
        let applied = store.apply("worker", &from, &to, None).unwrap();
        assert_eq!(applied.files, ["a.txt"]);

        std::fs::OpenOptions::new()
            .write(true)
            .open(&big)
            .unwrap()
            .set_len(MAX_TEXT_FILE_BYTES + 2)
            .unwrap();
        assert!(store
            .apply("worker", &from, &to, None)
            .unwrap_err()
            .contains("cannot be checkpointed"));
    }

    #[test]
    fn isolated_worker_refuses_a_nested_repository() {
        let lead = tmp("isolated-nested");
        let Some(worker) = lead_and_worker(&lead.0, &[("a.txt", "head\n")]) else {
            return;
        };
        let from = worker.to_string_lossy().into_owned();
        let to = lead.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("worker", &from, true).unwrap();
        std::fs::create_dir_all(worker.join("sub")).unwrap();
        assert!(git(&worker.join("sub"), &["init"]));
        std::fs::write(worker.join("sub/x.txt"), "nested\n").unwrap();

        let error = store.apply("worker", &from, &to, None).unwrap_err();
        assert!(
            error.contains("Cannot safely integrate sub/: it is a nested git repository"),
            "{error}"
        );
        assert!(!store.cleanup_safe("worker", &from).unwrap());
    }

    #[test]
    fn isolated_worker_refuses_a_lead_subfolder() {
        let lead = tmp("isolated-subfolder");
        let Some(worker) = lead_and_worker(&lead.0, &[("sub/a.txt", "head\n")]) else {
            return;
        };
        let from = worker.to_string_lossy().into_owned();
        let to = lead.0.join("sub").to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("worker", &from, true).unwrap();
        std::fs::write(worker.join("sub/a.txt"), "worker\n").unwrap();

        let error = store.apply("worker", &from, &to, None).unwrap_err();
        assert!(error.contains("not the top level"), "{error}");
        assert_eq!(read(&lead.0.join("sub/a.txt")), "head\n");
        assert!(!lead.0.join("sub/sub").exists());
    }

    #[cfg(unix)]
    #[test]
    fn isolated_worker_apply_keeps_lead_permission_bits() {
        use std::os::unix::fs::PermissionsExt;
        let set = |path: &Path, mode: u32| {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
        };
        let lead = tmp("isolated-modes");
        let Some(worker) = lead_and_worker(&lead.0, &[("run.sh", "head\n")]) else {
            return;
        };
        set(&lead.0.join("run.sh"), 0o640);
        let from = worker.to_string_lossy().into_owned();
        let to = lead.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("worker", &from, true).unwrap();

        std::fs::write(worker.join("run.sh"), "worker\n").unwrap();
        set(&worker.join("run.sh"), 0o777);
        std::fs::write(worker.join("new.sh"), "new\n").unwrap();
        set(&worker.join("new.sh"), 0o777);
        std::fs::write(worker.join("plain.txt"), "plain\n").unwrap();
        set(&worker.join("plain.txt"), 0o666);

        let applied = store.apply("worker", &from, &to, None).unwrap();
        assert_eq!(applied.files, ["new.sh", "plain.txt", "run.sh"]);
        assert_eq!(file_mode(&lead.0.join("run.sh")), Some(0o750));
        assert_eq!(file_mode(&lead.0.join("new.sh")), Some(0o755));
        assert_eq!(file_mode(&lead.0.join("plain.txt")), Some(0o644));
        let retried = store.apply("worker", &from, &to, None).unwrap();
        assert_eq!(retried.already_applied, 3);
    }

    #[test]
    fn write_changes_rechecks_each_target_before_writing() {
        let lead = tmp("write-recheck");
        std::fs::write(lead.0.join("a.txt"), "old\n").unwrap();
        std::fs::write(lead.0.join("b.txt"), "changed\n").unwrap();
        let mode = file_mode(&lead.0.join("a.txt"));
        let change = |relative: &str| WorkerChange {
            relative: relative.into(),
            before: (FileState::Contents(b"old\n".to_vec()), mode),
            after: (FileState::Contents(b"new\n".to_vec()), mode),
            head_before: false,
        };
        let changes = [change("a.txt"), change("b.txt")];

        let error = write_changes(&lead.0, &changes, &[], &[], true, None).unwrap_err();
        assert!(error.contains("lead checkout changed"), "{error}");
        assert_eq!(read(&lead.0.join("a.txt")), "new\n");
        assert_eq!(read(&lead.0.join("b.txt")), "changed\n");

        std::fs::write(lead.0.join("b.txt"), "old\n").unwrap();
        write_changes(&lead.0, &changes, &[], &[], true, None).unwrap();
        assert_eq!(read(&lead.0.join("a.txt")), "new\n");
        assert_eq!(read(&lead.0.join("b.txt")), "new\n");
    }

    #[test]
    fn isolated_worker_keep_all_still_applies() {
        let lead = tmp("isolated-keep");
        let Some(worker) = lead_and_worker(&lead.0, &[("a.txt", "head\n")]) else {
            return;
        };
        std::fs::write(worker.join("loose.txt"), "seed\n").unwrap();
        std::fs::write(lead.0.join("loose.txt"), "seed\n").unwrap();
        let from = worker.to_string_lossy().into_owned();
        let to = lead.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("worker", &from, true).unwrap();

        let edited = vec!["a.txt".to_string(), "loose.txt".to_string()];
        store.prepare("worker", &from, &edited).unwrap();
        std::fs::write(worker.join("a.txt"), "worker\n").unwrap();
        std::fs::write(worker.join("loose.txt"), "seed\nworker\n").unwrap();
        store.capture("worker", &from, &edited).unwrap();
        assert_eq!(store.status("worker", &from).unwrap().files.len(), 2);

        // Per-path keep releases the review entry but never the seed.
        store.keep("worker", &from, Some("loose.txt")).unwrap();
        let dir = store.session_dir("worker");
        let manifest = read_manifest(&dir).unwrap().unwrap();
        assert!(manifest.seed.contains_key("loose.txt"));

        store.keep("worker", &from, None).unwrap();
        assert!(store.status("worker", &from).unwrap().files.is_empty());
        let manifest = read_manifest(&dir).unwrap().unwrap();
        assert!(manifest.isolated);
        assert!(manifest.seed.contains_key("loose.txt"));
        assert!(manifest.touched.is_empty());
        assert!(!store.cleanup_safe("worker", &from).unwrap());

        let applied = store.apply("worker", &from, &to, None).unwrap();
        assert_eq!(applied.files, ["a.txt", "loose.txt"]);
        assert_eq!(read(&lead.0.join("a.txt")), "worker\n");
        assert_eq!(read(&lead.0.join("loose.txt")), "seed\nworker\n");
    }

    #[test]
    fn legacy_manifest_without_isolated_flag_keeps_tool_event_delta() {
        let manifest: Manifest = serde_json::from_str(r#"{"cwd":"/tmp","files":{}}"#).unwrap();
        assert!(!manifest.isolated);
        assert!(manifest.seed.is_empty());
    }

    /// True when `dir` is on a case-insensitive file system.
    fn case_insensitive(dir: &Path) -> bool {
        let probe = dir.join("CaseProbe");
        std::fs::write(&probe, "").unwrap();
        let insensitive = dir.join("caseprobe").exists();
        std::fs::remove_file(probe).unwrap();
        insensitive
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    fn rename(from: &str, to: &str) -> CheckpointRename {
        CheckpointRename {
            from: from.into(),
            to: to.into(),
        }
    }

    #[test]
    fn isolated_worker_applies_a_case_only_file_rename() {
        let lead = tmp("case-file");
        if !case_insensitive(&lead.0) {
            return;
        }
        let Some(worker) = lead_and_worker(&lead.0, &[("Foo.ts", "head\n"), ("keep.txt", "k\n")])
        else {
            return;
        };
        if !ignores_case(&worker) {
            return;
        }
        let from = worker.to_string_lossy().into_owned();
        let to = lead.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("worker", &from, true).unwrap();
        assert!(store.cleanup_safe("worker", &from).unwrap());

        std::fs::rename(worker.join("Foo.ts"), worker.join("foo.ts")).unwrap();
        assert!(!store.cleanup_safe("worker", &from).unwrap());

        let applied = store.apply("worker", &from, &to, None).unwrap();
        assert_eq!(applied.files, ["Foo.ts -> foo.ts"]);
        assert_eq!(applied.renamed, [rename("Foo.ts", "foo.ts")]);
        assert_eq!(applied.already_applied, 0);
        assert!(names(&lead.0).contains(&"foo.ts".to_string()));
        assert!(!names(&lead.0).contains(&"Foo.ts".to_string()));
        assert_eq!(read(&lead.0.join("foo.ts")), "head\n");

        let retried = store.apply("worker", &from, &to, None).unwrap();
        assert_eq!(retried.already_applied, 1);
    }

    #[test]
    fn isolated_worker_applies_a_case_only_folder_rename_once() {
        let lead = tmp("case-folder");
        if !case_insensitive(&lead.0) {
            return;
        }
        let Some(worker) =
            lead_and_worker(&lead.0, &[("Src/a.ts", "a\n"), ("Src/Lib/b.ts", "b\n")])
        else {
            return;
        };
        if !ignores_case(&worker) {
            return;
        }
        let from = worker.to_string_lossy().into_owned();
        let to = lead.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("worker", &from, true).unwrap();

        std::fs::rename(worker.join("Src"), worker.join("src")).unwrap();
        std::fs::rename(worker.join("src/Lib"), worker.join("src/lib")).unwrap();

        let applied = store.apply("worker", &from, &to, None).unwrap();
        assert_eq!(
            applied.renamed,
            [rename("Src", "src"), rename("Src/Lib", "src/lib")]
        );
        assert!(names(&lead.0).contains(&"src".to_string()));
        assert_eq!(names(&lead.0.join("src")), ["a.ts", "lib"]);
        assert_eq!(read(&lead.0.join("src/lib/b.ts")), "b\n");

        let retried = store.apply("worker", &from, &to, None).unwrap();
        assert_eq!(retried.already_applied, 2);

        // A lead that renamed the folder some other way is a conflict.
        let lead2 = tmp("case-folder-conflict");
        let Some(worker2) = lead_and_worker(&lead2.0, &[("Src/a.ts", "a\n")]) else {
            return;
        };
        let from2 = worker2.to_string_lossy().into_owned();
        let to2 = lead2.0.to_string_lossy().into_owned();
        store.ensure("worker2", &from2, true).unwrap();
        std::fs::rename(worker2.join("Src"), worker2.join("src")).unwrap();
        std::fs::rename(lead2.0.join("Src"), lead2.0.join("SRC")).unwrap();
        let error = store.apply("worker2", &from2, &to2, None).unwrap_err();
        assert!(
            error.contains("Cannot integrate Src: the lead checkout changed"),
            "{error}"
        );
    }

    #[test]
    fn isolated_worker_applies_a_case_rename_with_an_edit() {
        let lead = tmp("case-edit");
        if !case_insensitive(&lead.0) {
            return;
        }
        let Some(worker) = lead_and_worker(&lead.0, &[("Foo.ts", "head\n")]) else {
            return;
        };
        if !ignores_case(&worker) {
            return;
        }
        let from = worker.to_string_lossy().into_owned();
        let to = lead.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("worker", &from, true).unwrap();

        std::fs::rename(worker.join("Foo.ts"), worker.join("foo.ts")).unwrap();
        std::fs::write(worker.join("foo.ts"), "worker\n").unwrap();

        let applied = store.apply("worker", &from, &to, None).unwrap();
        assert_eq!(applied.files, ["Foo.ts", "Foo.ts -> foo.ts"]);
        assert_eq!(names(&lead.0), [".git", "foo.ts", "worker-tree"]);
        assert_eq!(read(&lead.0.join("foo.ts")), "worker\n");
        let retried = store.apply("worker", &from, &to, None).unwrap();
        assert_eq!(retried.already_applied, 2);
    }

    #[test]
    fn isolated_worker_ignores_line_endings_from_the_worker_config() {
        let lead = tmp("isolated-autocrlf");
        if !init_git_commit(&lead.0, &[("a.txt", "a\n"), ("b.txt", "b\n")]) {
            return;
        }
        // Set after the lead's checkout, so the lead has LF and the worker,
        // checked out later, has CRLF.
        assert!(git(&lead.0, &["config", "core.autocrlf", "true"]));
        let worker = lead.0.join("worker-tree");
        let worker_path = worker.to_string_lossy().into_owned();
        if !git(&lead.0, &["worktree", "add", "-b", "worker", &worker_path]) {
            return;
        }
        assert_eq!(read(&worker.join("a.txt")), "a\r\n");
        assert_eq!(read(&lead.0.join("a.txt")), "a\n");
        let from = worker_path;
        let to = lead.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("worker", &from, true).unwrap();

        std::fs::write(worker.join("a.txt"), "worker\r\n").unwrap();
        std::fs::write(worker.join("b.txt"), "worker b\r\n").unwrap();
        // A real lead edit is still a conflict.
        std::fs::write(lead.0.join("b.txt"), "lead\n").unwrap();
        let error = store.apply("worker", &from, &to, None).unwrap_err();
        assert!(error.contains("Cannot integrate b.txt"), "{error}");
        assert_eq!(read(&lead.0.join("a.txt")), "a\n");

        std::fs::write(lead.0.join("b.txt"), "b\n").unwrap();
        let applied = store.apply("worker", &from, &to, None).unwrap();
        assert_eq!(applied.files, ["a.txt", "b.txt"]);
        assert_eq!(read(&lead.0.join("a.txt")), "worker\r\n");
        assert_eq!(read(&lead.0.join("b.txt")), "worker b\r\n");
    }

    #[test]
    fn isolated_worker_ignores_line_endings_from_its_own_attributes() {
        let lead = tmp("isolated-eol-attr");
        let Some(worker) = lead_and_worker(
            &lead.0,
            &[
                (".gitattributes", "*.bin binary\n"),
                ("b.txt", "b\n"),
                ("c.txt", "c\n"),
            ],
        ) else {
            return;
        };
        let from = worker.to_string_lossy().into_owned();
        let to = lead.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("worker", &from, true).unwrap();

        std::fs::write(
            worker.join(".gitattributes"),
            "*.bin binary\n*.txt text eol=crlf\n",
        )
        .unwrap();
        std::fs::write(worker.join("b.txt"), "worker b\n").unwrap();
        // With the worker's attributes, HEAD's b.txt reads with CRLF.
        let delta = isolated_worker_delta(
            &store.session_dir("worker"),
            &worker,
            &read_manifest(&store.session_dir("worker"))
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        let b = delta
            .changes
            .iter()
            .find(|change| change.relative == "b.txt")
            .unwrap();
        assert_eq!(b.before.0, FileState::Contents(b"b\r\n".to_vec()));
        assert!(b.head_before);

        let applied = store.apply("worker", &from, &to, None).unwrap();
        assert_eq!(applied.files, [".gitattributes", "b.txt"]);
        assert_eq!(read(&lead.0.join("b.txt")), "worker b\n");
        assert_eq!(read(&lead.0.join("c.txt")), "c\n");
        assert_eq!(
            read(&lead.0.join(".gitattributes")),
            "*.bin binary\n*.txt text eol=crlf\n"
        );
    }

    #[test]
    fn isolated_worker_replaces_a_file_with_a_folder() {
        let lead = tmp("isolated-file-to-folder");
        let Some(worker) = lead_and_worker(&lead.0, &[("a", "head\n"), ("keep.txt", "k\n")]) else {
            return;
        };
        let from = worker.to_string_lossy().into_owned();
        let to = lead.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("worker", &from, true).unwrap();

        std::fs::remove_file(worker.join("a")).unwrap();
        std::fs::create_dir(worker.join("a")).unwrap();
        std::fs::write(worker.join("a/b"), "x\n").unwrap();

        let applied = store.apply("worker", &from, &to, None).unwrap();
        assert_eq!(applied.files, ["a", "a/b"]);
        assert_eq!(applied.already_applied, 0);
        assert!(lead.0.join("a").is_dir());
        assert_eq!(read(&lead.0.join("a/b")), "x\n");

        let retried = store.apply("worker", &from, &to, None).unwrap();
        assert_eq!(retried.files, applied.files);
        assert_eq!(retried.already_applied, 2);
    }

    #[test]
    fn isolated_worker_replaces_a_folder_with_a_file() {
        let lead = tmp("isolated-folder-to-file");
        let Some(worker) = lead_and_worker(
            &lead.0,
            &[
                ("d/x", "x\n"),
                ("d/sub/y", "y\n"),
                ("e/only.txt", "only\n"),
                ("keep.txt", "k\n"),
            ],
        ) else {
            return;
        };
        let from = worker.to_string_lossy().into_owned();
        let to = lead.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("worker", &from, true).unwrap();

        std::fs::remove_dir_all(worker.join("d")).unwrap();
        std::fs::write(worker.join("d"), "file\n").unwrap();
        // Deleting the last file in a folder removes the folder too.
        std::fs::remove_dir_all(worker.join("e")).unwrap();

        // An untracked file the lead keeps in the folder blocks the swap.
        std::fs::write(lead.0.join("d/sub/local"), "lead\n").unwrap();
        let error = store.apply("worker", &from, &to, None).unwrap_err();
        assert!(
            error.contains("Cannot integrate d: the lead checkout changed"),
            "{error}"
        );
        assert_eq!(read(&lead.0.join("d/x")), "x\n");
        std::fs::remove_file(lead.0.join("d/sub/local")).unwrap();

        let applied = store.apply("worker", &from, &to, None).unwrap();
        assert_eq!(applied.files, ["d", "d/sub/y", "d/x", "e/only.txt"]);
        assert_eq!(read(&lead.0.join("d")), "file\n");
        assert!(!lead.0.join("e").exists());
        assert_eq!(read(&lead.0.join("keep.txt")), "k\n");

        let retried = store.apply("worker", &from, &to, None).unwrap();
        assert_eq!(retried.files, applied.files);
        assert_eq!(retried.already_applied, 4);
    }

    #[cfg(unix)]
    #[test]
    fn path_contains_symlink_treats_a_file_parent_as_absent() {
        let root = tmp("symlink-parent");
        std::fs::write(root.0.join("file"), "x\n").unwrap();
        assert!(!path_contains_symlink(&root.0, "file/child"));
        assert!(!path_contains_symlink(&root.0, "missing/child"));
        std::fs::create_dir(root.0.join("real")).unwrap();
        std::os::unix::fs::symlink("real", root.0.join("link")).unwrap();
        assert!(path_contains_symlink(&root.0, "link/child"));
        assert!(path_contains_symlink(&root.0, "link"));
    }

    fn temp_files(dir: &Path) -> Vec<String> {
        names(dir)
            .into_iter()
            .filter(|name| name.contains(".monocode-"))
            .collect()
    }

    #[cfg(unix)]
    #[test]
    fn write_file_atomic_replaces_the_target_and_leaves_no_temp_file() {
        use std::os::unix::fs::PermissionsExt;
        let root = tmp("atomic");
        let target = root.0.join("dir/a.txt");
        write_file_atomic(&target, b"one\n", Some(0o750)).unwrap();
        assert_eq!(read(&target), "one\n");
        assert_eq!(file_mode(&target), Some(0o750));
        assert!(temp_files(&root.0.join("dir")).is_empty());

        // Without a mode, the existing file keeps its own.
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o640)).unwrap();
        write_file_atomic(&target, b"two\n", None).unwrap();
        assert_eq!(read(&target), "two\n");
        assert_eq!(file_mode(&target), Some(0o640));

        // A folder at the target fails at the rename, after the temp file
        // was written.
        std::fs::create_dir_all(root.0.join("dir/folder/inner")).unwrap();
        let error = write_file_atomic(&root.0.join("dir/folder"), b"x\n", None).unwrap_err();
        assert!(error.contains("is a directory"), "{error}");
        assert!(temp_files(&root.0.join("dir")).is_empty());

        // A read-only folder fails before or at the temp file.
        struct Restore(PathBuf);
        impl Drop for Restore {
            fn drop(&mut self) {
                let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
            }
        }
        let locked = root.0.join("dir");
        let _restore = Restore(locked.clone());
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
        if std::fs::write(locked.join("probe"), "").is_ok() {
            // Running as root, where permissions do not stop writes.
            return;
        }
        assert!(write_file_atomic(&target, b"three\n", None).is_err());
        assert_eq!(read(&target), "two\n");
        assert!(temp_files(&locked).is_empty());
    }

    #[test]
    fn write_changes_stops_when_the_worker_keeps_writing() {
        let lead = tmp("worker-busy");
        let Some(worker) = lead_and_worker(&lead.0, &[("a.txt", "a\n"), ("b.txt", "b\n")]) else {
            return;
        };
        let from = worker.to_string_lossy().into_owned();
        let to = lead.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("worker", &from, true).unwrap();
        std::fs::write(worker.join("a.txt"), "worker a\n").unwrap();
        std::fs::write(worker.join("b.txt"), "worker b\n").unwrap();

        let dir = store.session_dir("worker");
        let manifest = read_manifest(&dir).unwrap().unwrap();
        let delta = isolated_worker_delta(&dir, &worker, &manifest).unwrap();
        std::fs::write(worker.join("b.txt"), "worker b again\n").unwrap();
        let error =
            write_changes(&lead.0, &delta.changes, &[], &[], true, Some(&worker)).unwrap_err();
        assert_eq!(
            error,
            "The worker is still changing b.txt. Stop it and accept again. The worker worktree was kept."
        );
        assert_eq!(read(&lead.0.join("a.txt")), "worker a\n");
        assert_eq!(read(&lead.0.join("b.txt")), "b\n");

        // A path that appears after the writes is caught too.
        let delta = isolated_worker_delta(&dir, &worker, &manifest).unwrap();
        write_changes(&lead.0, &delta.changes, &[], &[], true, Some(&worker)).unwrap();
        std::fs::write(worker.join("late.txt"), "late\n").unwrap();
        let error = verify_worker_settled(&dir, &worker, &manifest, &delta).unwrap_err();
        assert!(error.contains("still changing late.txt"), "{error}");

        std::fs::remove_file(worker.join("late.txt")).unwrap();
        let applied = store.apply("worker", &from, &to, None).unwrap();
        assert_eq!(applied.files, ["a.txt", "b.txt"]);
        assert_eq!(read(&lead.0.join("b.txt")), "worker b again\n");
    }

    #[test]
    fn isolated_worker_reports_ignored_files_it_created() {
        let lead = tmp("isolated-ignored-report");
        let Some(worker) = lead_and_worker(
            &lead.0,
            &[(".gitignore", ".env*\nnode_modules/\n"), ("a.txt", "a\n")],
        ) else {
            return;
        };
        std::fs::create_dir_all(lead.0.join("node_modules/pkg")).unwrap();
        std::fs::write(lead.0.join("node_modules/pkg/index.js"), "lead\n").unwrap();
        let from = worker.to_string_lossy().into_owned();
        let to = lead.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("worker", &from, true).unwrap();
        assert!(store.cleanup_safe("worker", &from).unwrap());

        std::fs::write(worker.join(".env.local"), "SECRET=1\n").unwrap();
        std::fs::write(worker.join(".env.same"), "same\n").unwrap();
        std::fs::write(lead.0.join(".env.same"), "same\n").unwrap();
        std::fs::create_dir_all(worker.join("node_modules")).unwrap();
        std::fs::write(worker.join("node_modules/x"), "worker\n").unwrap();
        std::fs::write(worker.join("a.txt"), "worker\n").unwrap();
        assert!(!store.cleanup_safe("worker", &from).unwrap());

        let applied = store.apply("worker", &from, &to, None).unwrap();
        assert_eq!(applied.files, ["a.txt"]);
        assert_eq!(applied.ignored, [".env.local"]);
        assert!(!lead.0.join(".env.local").exists());
        assert!(!lead.0.join("node_modules/x").exists());

        std::fs::remove_dir_all(lead.0.join("node_modules")).unwrap();
        let applied = store.apply("worker", &from, &to, None).unwrap();
        assert_eq!(applied.ignored, [".env.local", "node_modules/"]);
        assert!(!lead.0.join("node_modules").exists());

        // Only the worker's own ignored files count.
        std::fs::remove_file(worker.join(".env.local")).unwrap();
        std::fs::remove_file(worker.join(".env.same")).unwrap();
        std::fs::remove_dir_all(worker.join("node_modules")).unwrap();
        std::fs::write(worker.join("a.txt"), "a\n").unwrap();
        assert!(store.cleanup_safe("worker", &from).unwrap());
    }

    #[test]
    fn isolated_worker_applies_a_staged_case_rename() {
        let lead = tmp("case-staged");
        if !case_insensitive(&lead.0) {
            return;
        }
        let Some(worker) = lead_and_worker(&lead.0, &[("Foo.ts", "head\n")]) else {
            return;
        };
        if !ignores_case(&worker) {
            return;
        }
        let from = worker.to_string_lossy().into_owned();
        let to = lead.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("worker", &from, true).unwrap();
        assert!(git(&worker, &["mv", "Foo.ts", "foo.ts"]));
        assert!(!store.cleanup_safe("worker", &from).unwrap());

        let applied = store.apply("worker", &from, &to, None).unwrap();
        assert_eq!(applied.files, ["Foo.ts -> foo.ts"]);
        assert_eq!(applied.renamed, [rename("Foo.ts", "foo.ts")]);
        assert_eq!(names(&lead.0), [".git", "foo.ts", "worker-tree"]);
        assert_eq!(read(&lead.0.join("foo.ts")), "head\n");
        let retried = store.apply("worker", &from, &to, None).unwrap();
        assert_eq!(retried.already_applied, 1);
    }

    #[test]
    fn isolated_worker_applies_a_staged_case_rename_with_an_edit() {
        let lead = tmp("case-staged-edit");
        if !case_insensitive(&lead.0) {
            return;
        }
        let Some(worker) = lead_and_worker(&lead.0, &[("Foo.ts", "head\n")]) else {
            return;
        };
        if !ignores_case(&worker) {
            return;
        }
        let from = worker.to_string_lossy().into_owned();
        let to = lead.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("worker", &from, true).unwrap();
        assert!(git(&worker, &["mv", "Foo.ts", "foo.ts"]));
        std::fs::write(worker.join("foo.ts"), "worker\n").unwrap();

        let applied = store.apply("worker", &from, &to, None).unwrap();
        assert_eq!(applied.files, ["Foo.ts", "Foo.ts -> foo.ts"]);
        assert_eq!(names(&lead.0), [".git", "foo.ts", "worker-tree"]);
        assert_eq!(read(&lead.0.join("foo.ts")), "worker\n");
        let retried = store.apply("worker", &from, &to, None).unwrap();
        assert_eq!(retried.already_applied, 2);
    }

    #[test]
    fn isolated_worker_resumes_a_case_rename_stopped_halfway() {
        let lead = tmp("case-resume");
        if !case_insensitive(&lead.0) {
            return;
        }
        let Some(worker) = lead_and_worker(&lead.0, &[("Foo.ts", "head\n")]) else {
            return;
        };
        if !ignores_case(&worker) {
            return;
        }
        let from = worker.to_string_lossy().into_owned();
        let to = lead.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("worker", &from, true).unwrap();
        std::fs::rename(worker.join("Foo.ts"), worker.join("foo.ts")).unwrap();
        std::fs::write(worker.join("foo.ts"), "worker\n").unwrap();

        // The app stopped after the first step of `rename_case`.
        let temp = lead.0.join(".foo.ts.monocode-case.tmp");
        std::fs::rename(lead.0.join("Foo.ts"), &temp).unwrap();

        // A temporary file with other contents is not resumed.
        std::fs::write(&temp, "someone else\n").unwrap();
        let error = store.apply("worker", &from, &to, None).unwrap_err();
        assert!(
            error.contains("Cannot integrate Foo.ts: the lead checkout changed"),
            "{error}"
        );
        assert_eq!(read(&temp), "someone else\n");

        std::fs::write(&temp, "head\n").unwrap();
        let applied = store.apply("worker", &from, &to, None).unwrap();
        assert_eq!(applied.files, ["Foo.ts", "Foo.ts -> foo.ts"]);
        assert_eq!(names(&lead.0), [".git", "foo.ts", "worker-tree"]);
        assert_eq!(read(&lead.0.join("foo.ts")), "worker\n");
    }

    #[test]
    fn isolated_worker_refuses_an_unconfirmed_folder_rename() {
        let lead = tmp("case-unconfirmed");
        if !case_insensitive(&lead.0) {
            return;
        }
        let Some(worker) = lead_and_worker(&lead.0, &[("Src/a.ts", "a\n"), ("keep.txt", "k\n")])
        else {
            return;
        };
        if !ignores_case(&worker) {
            return;
        }
        let from = worker.to_string_lossy().into_owned();
        let to = lead.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("worker", &from, true).unwrap();
        // No tracked file survives in the folder to confirm the rename.
        std::fs::remove_file(worker.join("Src/a.ts")).unwrap();
        std::fs::rename(worker.join("Src"), worker.join("src")).unwrap();
        std::fs::write(worker.join("src/new.ts"), "new\n").unwrap();

        // The lead keeps a file of its own in the folder, so its final
        // spelling cannot be known.
        std::fs::write(lead.0.join("Src/local.txt"), "lead\n").unwrap();
        let error = store.apply("worker", &from, &to, None).unwrap_err();
        assert_eq!(
            error,
            "Cannot tell how the worker renamed Src. The worker worktree was kept."
        );
        assert_eq!(read(&lead.0.join("Src/a.ts")), "a\n");
        assert!(!lead.0.join("Src/new.ts").exists());

        // Once the lead's folder empties out, the worker's spelling is safe.
        std::fs::remove_file(lead.0.join("Src/local.txt")).unwrap();
        let applied = store.apply("worker", &from, &to, None).unwrap();
        assert_eq!(applied.files, ["Src/a.ts", "src/new.ts"]);
        assert_eq!(names(&lead.0), [".git", "keep.txt", "src", "worker-tree"]);
        assert_eq!(names(&lead.0.join("src")), ["new.ts"]);
        let retried = store.apply("worker", &from, &to, None).unwrap();
        assert_eq!(retried.already_applied, 2);
    }

    #[test]
    fn isolated_worker_keep_all_keeps_tool_written_ignored_files() {
        let lead = tmp("isolated-keep-ignored");
        let Some(worker) = lead_and_worker(&lead.0, &[(".gitignore", ".env\n"), ("a.txt", "a\n")])
        else {
            return;
        };
        let from = worker.to_string_lossy().into_owned();
        let to = lead.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("worker", &from, true).unwrap();
        store.prepare("worker", &from, &[".env".into()]).unwrap();
        std::fs::write(worker.join(".env"), "TOKEN=1\n").unwrap();
        store.capture("worker", &from, &[".env".into()]).unwrap();

        let applied = store.apply("worker", &from, &to, None).unwrap();
        assert_eq!(applied.files, [".env"]);
        assert!(applied.ignored.is_empty());
        assert_eq!(read(&lead.0.join(".env")), "TOKEN=1\n");

        // Keep all drops the tool attribution, so `.env` now counts as a
        // worker-created ignored file. The lead has it identically, so it is
        // not reported, but cleanup still keeps the worktree.
        store.keep("worker", &from, None).unwrap();
        let retried = store.apply("worker", &from, &to, None).unwrap();
        assert!(retried.files.is_empty());
        assert!(retried.ignored.is_empty());
        assert_eq!(read(&lead.0.join(".env")), "TOKEN=1\n");
        assert_eq!(read(&worker.join(".env")), "TOKEN=1\n");
        assert!(!store.cleanup_safe("worker", &from).unwrap());
    }

    #[test]
    fn rejects_invalid_session_id() {
        let err = validate_id("../x", "session").unwrap_err();
        assert!(err.contains("Invalid"));
    }
}
