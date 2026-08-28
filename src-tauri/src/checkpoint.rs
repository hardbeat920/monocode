use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use crate::fs::{
    expand_home, git_checked, git_diff_files_for, git_file_diff_for, resolve_repo_path,
    GitChangedFile, GitDiffIndex, GitDiffStats, GitFileDiff, MAX_TEXT_FILE_BYTES,
};

const MAX_SNAPSHOT_FILES: usize = 500;

#[derive(Clone)]
pub struct CheckpointStore {
    root: PathBuf,
}

impl CheckpointStore {
    fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn session_dir(&self, session_id: &str) -> PathBuf {
        self.root.join(session_id)
    }

    fn ensure(&self, session_id: &str, cwd: &str) -> Result<(), String> {
        let root = project_root(cwd)?;
        let dir = self.session_dir(session_id);
        if let Some(manifest) = read_manifest(&dir)? {
            if same_cwd(&manifest.cwd, cwd) {
                return Ok(());
            }
            let _ = std::fs::remove_dir_all(&dir);
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
            },
        )
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
            if manifest.touched.insert(relative.clone()) {
                dirty = true;
            }
            let tracked_in_head = in_head(&root, &relative);
            if tracked_in_head && manifest.tracked.insert(relative.clone()) {
                dirty = true;
            }
            if manifest.files.contains_key(&relative) {
                continue;
            }
            // Only snapshot files that do not exist yet so a late capture cannot
            // freeze the agent's write as the baseline.
            // A missing tracked file must continue to use HEAD as its baseline.
            if root.join(&relative).exists() || tracked_in_head {
                continue;
            }
            manifest
                .files
                .insert(relative.clone(), snapshot_file(&dir, &root, &relative)?);
            dirty = true;
        }
        if dirty {
            write_manifest(&dir, &manifest)?;
        }
        Ok(())
    }

    fn sync(&self, session_id: &str, cwd: &str) -> Result<(), String> {
        let Some(mut manifest) = self.load_matching(session_id, cwd)? else {
            return Ok(());
        };
        let root = project_root(cwd)?;
        let dir = self.session_dir(session_id);
        let index = git_diff_files_for(&root);
        let foreign_touched = self.foreign_touched_paths(cwd, session_id);
        let git_dirty: HashSet<&str> = index
            .files
            .iter()
            .map(|file| file.relative.as_str())
            .collect();
        let mut dirty = false;
        for file in &index.files {
            if manifest.touched.len() >= MAX_SNAPSHOT_FILES {
                break;
            }
            let Ok(relative) = resolve_repo_path(&root, &file.relative) else {
                continue;
            };
            if foreign_touched.contains(&relative) {
                continue;
            }
            // Already-dirty at ensure(): only capture() may mark these.
            if manifest.files.contains_key(&relative) {
                continue;
            }
            if matches!(
                session_change(&dir, &root, &manifest, &relative, &git_dirty),
                Change::None
            ) {
                continue;
            }
            if in_head(&root, &relative) && manifest.tracked.insert(relative.clone()) {
                dirty = true;
            }
            if manifest.touched.insert(relative) {
                dirty = true;
            }
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
        Ok(diff_from_manifest(
            &self.session_dir(session_id),
            &root,
            &manifest,
        ))
    }

    /// Remaining git line counts for each session, using one working-tree index.
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
            let status =
                diff_from_manifest_with(&index, &self.session_dir(session_id), &root, &manifest);
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
        let Some(manifest) = self.load_matching(session_id, cwd)? else {
            return Ok(CheckpointStatus { files: Vec::new() });
        };
        let root = project_root(cwd)?;
        let dir = self.session_dir(session_id);
        let changed = diff_from_manifest(&dir, &root, &manifest);
        if let Some(relative) = relative {
            let relative = resolve_repo_path(&root, relative)?;
            restore_one(&dir, &root, &manifest, &relative)?;
            return self.status(session_id, cwd);
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
            let _ = std::fs::remove_dir_all(&dir);
            return Ok(CheckpointStatus { files: Vec::new() });
        };
        let relative = resolve_repo_path(&root, relative)?;
        manifest
            .files
            .insert(relative.clone(), snapshot_file(&dir, &root, &relative)?);
        write_manifest(&dir, &manifest)?;
        self.status(session_id, cwd)
    }

    /// Diff base for review: the session's snapshot when it has one, else HEAD
    /// (the path was clean at session start, so HEAD is that same baseline).
    fn file_diff(
        &self,
        session_id: &str,
        cwd: &str,
        relative: &str,
    ) -> Result<GitFileDiff, String> {
        let root = project_root(cwd)?;
        let relative = resolve_repo_path(&root, relative)?;
        let kind = self
            .load_matching(session_id, cwd)?
            .and_then(|manifest| manifest.files.get(&relative).copied());
        match kind {
            Some(kind) => Ok(checkpoint_file_diff(
                &self.session_dir(session_id),
                &root,
                &relative,
                kind,
            )),
            None => git_file_diff_for(&root, &relative),
        }
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
            paths.extend(manifest.touched.iter().cloned());
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
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointStatus {
    pub files: Vec<CheckpointFile>,
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
) -> Result<(), String> {
    validate_id(&session_id, "session")?;
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || store.ensure(&session_id, &cwd))
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
    tauri::async_runtime::spawn_blocking(move || store.capture(&session_id, &cwd, &paths))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn session_checkpoint_sync(
    store: State<'_, CheckpointStore>,
    session_id: String,
    cwd: String,
) -> Result<(), String> {
    validate_id(&session_id, "session")?;
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || store.sync(&session_id, &cwd))
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
    tauri::async_runtime::spawn_blocking(move || store.status(&session_id, &cwd))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn session_checkpoint_stats(
    store: State<'_, CheckpointStore>,
    cwd: String,
    session_ids: Vec<String>,
) -> Result<HashMap<String, GitDiffStats>, String> {
    for session_id in &session_ids {
        validate_id(session_id, "session")?;
    }
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || store.stats_for_sessions(&cwd, &session_ids))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn session_checkpoint_file_diff(
    store: State<'_, CheckpointStore>,
    session_id: String,
    cwd: String,
    relative: String,
) -> Result<GitFileDiff, String> {
    validate_id(&session_id, "session")?;
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || store.file_diff(&session_id, &cwd, &relative))
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
    tauri::async_runtime::spawn_blocking(move || store.undo(&session_id, &cwd, relative.as_deref()))
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
    tauri::async_runtime::spawn_blocking(move || store.keep(&session_id, &cwd, relative.as_deref()))
        .await
        .map_err(|e| e.to_string())?
}

fn diff_from_manifest(dir: &Path, root: &Path, manifest: &Manifest) -> CheckpointStatus {
    diff_from_manifest_with(&git_diff_files_for(root), dir, root, manifest)
}

fn diff_from_manifest_with(
    index: &GitDiffIndex,
    dir: &Path,
    root: &Path,
    manifest: &Manifest,
) -> CheckpointStatus {
    let by_relative: BTreeMap<&str, &GitChangedFile> = index
        .files
        .iter()
        .map(|file| (file.relative.as_str(), file))
        .collect();
    let git_dirty: HashSet<&str> = by_relative.keys().copied().collect();
    let mut files = Vec::new();

    for relative in &manifest.touched {
        match session_change(dir, root, manifest, relative, &git_dirty) {
            Change::None => continue,
            Change::Snapshot(before, after) => {
                files.push(describe_snapshot_change(root, relative, &before, &after));
            }
            Change::Head => files.push(describe_head_change(
                root,
                relative,
                by_relative.get(relative.as_str()).copied(),
            )),
        }
    }

    files.sort_by(|a, b| a.relative.cmp(&b.relative));
    CheckpointStatus { files }
}

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

/// What this session did to one path, and against which baseline.
enum Change {
    None,
    /// Snapshotted because the path was already dirty at session start, so its
    /// baseline is the snapshot. Both sides are carried to avoid a second read.
    Snapshot(FileState, FileState),
    /// Clean at session start, so HEAD is the baseline and git's counts apply.
    Head,
}

fn session_change(
    dir: &Path,
    root: &Path,
    manifest: &Manifest,
    relative: &str,
    git_dirty: &HashSet<&str>,
) -> Change {
    // Once a tracked path is clean against HEAD, its session change was
    // committed (or otherwise resolved) and no longer needs review.
    if !git_dirty.contains(relative)
        && (manifest.tracked.contains(relative) || in_head(root, relative))
    {
        return Change::None;
    }
    match manifest.files.get(relative) {
        Some(SnapshotKind::Skipped) => Change::None,
        Some(kind) => {
            let before = read_snapshot(dir, relative, *kind);
            let after = read_worktree(root, relative);
            if before == after {
                return Change::None;
            }
            Change::Snapshot(before, after)
        }
        None => {
            let changed = git_dirty.contains(relative)
                || (root.join(relative).is_file() && !in_head(root, relative));
            if changed {
                Change::Head
            } else {
                Change::None
            }
        }
    }
}

fn describe_snapshot_change(
    root: &Path,
    relative: &str,
    before: &FileState,
    after: &FileState,
) -> CheckpointFile {
    let (additions, deletions) = line_diff_counts(before, after);
    CheckpointFile {
        path: root.join(relative).to_string_lossy().into_owned(),
        relative: relative.to_string(),
        status: change_status(before, after).into(),
        additions,
        deletions,
    }
}

fn describe_head_change(
    root: &Path,
    relative: &str,
    git: Option<&GitChangedFile>,
) -> CheckpointFile {
    if let Some(file) = git {
        return CheckpointFile {
            path: file.path.clone(),
            relative: file.relative.clone(),
            status: file.status.clone(),
            additions: file.additions,
            deletions: file.deletions,
        };
    }
    let abs = root.join(relative);
    let status = if !abs.exists() { "deleted" } else { "modified" };
    CheckpointFile {
        path: abs.to_string_lossy().into_owned(),
        relative: relative.to_string(),
        status: status.into(),
        additions: 0,
        deletions: 0,
    }
}

fn change_status(before: &FileState, after: &FileState) -> &'static str {
    match (before, after) {
        (FileState::Missing, _) => "added",
        (_, FileState::Missing) => "deleted",
        _ => "modified",
    }
}

fn state_bytes(state: &FileState) -> &[u8] {
    match state {
        FileState::Contents(bytes) => bytes,
        FileState::Missing | FileState::Skipped => &[],
    }
}

/// Baseline vs worktree contents for one file, shaped like the HEAD diff so the
/// editor can render either base with the same merge view.
fn checkpoint_file_diff(
    dir: &Path,
    root: &Path,
    relative: &str,
    kind: SnapshotKind,
) -> GitFileDiff {
    let before = read_snapshot(dir, relative, kind);
    let after = read_worktree(root, relative);
    let status = change_status(&before, &after);
    let too_large = matches!(before, FileState::Skipped) || matches!(after, FileState::Skipped);
    let binary = state_bytes(&before).contains(&0) || state_bytes(&after).contains(&0);
    let (original, current) = if binary || too_large {
        (String::new(), String::new())
    } else {
        (
            String::from_utf8_lossy(state_bytes(&before)).into_owned(),
            String::from_utf8_lossy(state_bytes(&after)).into_owned(),
        )
    };
    GitFileDiff {
        path: root.join(relative).to_string_lossy().into_owned(),
        relative: relative.to_string(),
        status: status.to_string(),
        original,
        current,
        binary,
        too_large,
    }
}

/// LCS cells we will fill before calling an edit a whole-file rewrite. Keeps a
/// reformatted 8 MB file from stalling the status poll.
const MAX_DIFF_CELLS: usize = 4_000_000;

fn line_diff_counts(before: &FileState, after: &FileState) -> (i64, i64) {
    if matches!(before, FileState::Skipped) || matches!(after, FileState::Skipped) {
        return (0, 0);
    }
    let before = state_bytes(before);
    let after = state_bytes(after);
    if before.contains(&0) || after.contains(&0) {
        return (0, 0);
    }
    count_line_changes(&split_lines(before), &split_lines(after))
}

fn split_lines(bytes: &[u8]) -> Vec<&[u8]> {
    if bytes.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<&[u8]> = bytes.split(|byte| *byte == b'\n').collect();
    if lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    lines
}

fn count_line_changes(before: &[&[u8]], after: &[&[u8]]) -> (i64, i64) {
    let mut head = 0;
    while head < before.len() && head < after.len() && before[head] == after[head] {
        head += 1;
    }
    let mut tail = 0;
    while tail < before.len() - head
        && tail < after.len() - head
        && before[before.len() - 1 - tail] == after[after.len() - 1 - tail]
    {
        tail += 1;
    }
    let before = &before[head..before.len() - tail];
    let after = &after[head..after.len() - tail];
    if before.is_empty()
        || after.is_empty()
        || before.len().saturating_mul(after.len()) > MAX_DIFF_CELLS
    {
        return (after.len() as i64, before.len() as i64);
    }
    let common = lcs_len(before, after);
    (
        (after.len() - common) as i64,
        (before.len() - common) as i64,
    )
}

fn lcs_len(a: &[&[u8]], b: &[&[u8]]) -> usize {
    // Roll over the shorter side so the two rows stay bounded by MAX_DIFF_CELLS.
    let (long, short) = if a.len() >= b.len() { (a, b) } else { (b, a) };
    let mut prev = vec![0usize; short.len() + 1];
    let mut row = vec![0usize; short.len() + 1];
    for left in long {
        for (index, right) in short.iter().enumerate() {
            row[index + 1] = if left == right {
                prev[index] + 1
            } else {
                row[index].max(prev[index + 1])
            };
        }
        std::mem::swap(&mut prev, &mut row);
    }
    prev[short.len()]
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
            write_worktree(&root.join(relative), &bytes)?;
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
    let abs = root.join(relative);
    if !abs.exists() {
        return Ok(SnapshotKind::Missing);
    }
    if !abs.is_file() {
        return Ok(SnapshotKind::Skipped);
    }
    let meta = std::fs::metadata(&abs).map_err(|e| e.to_string())?;
    if meta.len() > MAX_TEXT_FILE_BYTES {
        return Ok(SnapshotKind::Skipped);
    }
    let bytes = std::fs::read(&abs).map_err(|e| e.to_string())?;
    let blob = blob_path(dir, relative)?;
    if let Some(parent) = blob.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&blob, bytes).map_err(|e| e.to_string())?;
    Ok(SnapshotKind::Contents)
}

fn read_snapshot(dir: &Path, relative: &str, kind: SnapshotKind) -> FileState {
    match kind {
        SnapshotKind::Missing => FileState::Missing,
        SnapshotKind::Skipped => FileState::Skipped,
        SnapshotKind::Contents => match blob_path(dir, relative)
            .ok()
            .and_then(|path| std::fs::read(path).ok())
        {
            Some(bytes) => FileState::Contents(bytes),
            None => FileState::Missing,
        },
    }
}

fn read_worktree(root: &Path, relative: &str) -> FileState {
    let abs = root.join(relative);
    if !abs.exists() {
        return FileState::Missing;
    }
    if !abs.is_file() {
        return FileState::Skipped;
    }
    match std::fs::metadata(&abs).and_then(|meta| {
        if meta.len() > MAX_TEXT_FILE_BYTES {
            return Ok(FileState::Skipped);
        }
        std::fs::read(&abs).map(FileState::Contents)
    }) {
        Ok(state) => state,
        Err(_) => FileState::Missing,
    }
}

fn write_worktree(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if path.is_dir() {
        return Err(format!("{} is a directory", path.display()));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, bytes).map_err(|e| e.to_string())
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

fn blob_path(dir: &Path, relative: &str) -> Result<PathBuf, String> {
    if relative.is_empty()
        || relative.starts_with('/')
        || relative
            .split('/')
            .any(|part| part.is_empty() || part == "..")
    {
        return Err("Invalid path".into());
    }
    Ok(dir.join("files").join(relative))
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
        store.sync(id, cwd).unwrap();
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

        store.ensure("s1", &cwd).unwrap();

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
        store.ensure("s1", &cwd).unwrap();

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
        store.ensure("s1", &cwd).unwrap();
        std::fs::write(repo.0.join("a.txt"), "agent-1\n").unwrap();
        record(&store, "s1", &cwd, &["a.txt"]);
        store.ensure("s1", &cwd).unwrap();
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
        store.ensure("s1", &cwd).unwrap();
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
        store.ensure("s1", &cwd).unwrap();
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

        store.ensure("s1", &cwd).unwrap();
        std::fs::write(repo.0.join("plan.md"), "session-one\n").unwrap();
        record(&store, "s1", &cwd, &["plan.md"]);

        store.ensure("s2", &cwd).unwrap();
        store.sync("s2", &cwd).unwrap();
        assert!(store.status("s2", &cwd).unwrap().files.is_empty());
    }

    #[test]
    fn sync_does_not_claim_other_session_edits() {
        let repo = tmp("sync-other-session");
        if !init_git_commit(&repo.0, &[("plan.md", "old\n"), ("readme.md", "old\n")]) {
            return;
        }
        let cwd = repo.0.to_string_lossy().into_owned();
        let (_root, store) = store();

        store.ensure("s1", &cwd).unwrap();
        store.ensure("s2", &cwd).unwrap();

        std::fs::write(repo.0.join("plan.md"), "session-one\n").unwrap();
        record(&store, "s1", &cwd, &["plan.md"]);

        store.sync("s2", &cwd).unwrap();
        assert_eq!(
            relatives(&store.status("s2", &cwd).unwrap()),
            Vec::<&str>::new()
        );
        assert_eq!(
            relatives(&store.status("s1", &cwd).unwrap()),
            vec!["plan.md"]
        );
    }

    #[test]
    fn other_session_edits_do_not_appear_in_review() {
        let repo = tmp("two-sessions");
        if !init_git_commit(&repo.0, &[("plan.md", "old\n"), ("readme.md", "old\n")]) {
            return;
        }
        let cwd = repo.0.to_string_lossy().into_owned();
        let (_root, store) = store();

        store.ensure("s1", &cwd).unwrap();
        std::fs::write(repo.0.join("plan.md"), "session-one\n").unwrap();
        record(&store, "s1", &cwd, &["plan.md"]);
        assert_eq!(
            relatives(&store.status("s1", &cwd).unwrap()),
            vec!["plan.md"]
        );

        store.ensure("s2", &cwd).unwrap();
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
    fn capture_missing_path_lets_non_git_undo_delete() {
        let project = tmp("nongit");
        let cwd = project.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("s1", &cwd).unwrap();
        store
            .capture(
                "s1",
                &cwd,
                &[project.0.join("made.txt").to_string_lossy().into_owned()],
            )
            .unwrap();
        std::fs::write(project.0.join("made.txt"), "hello\n").unwrap();
        assert_eq!(
            relatives(&store.status("s1", &cwd).unwrap()),
            vec!["made.txt"]
        );
        store.undo("s1", &cwd, None).unwrap();
        assert!(!project.0.join("made.txt").exists());
    }

    #[test]
    fn capture_does_not_overwrite_existing_file_after_write() {
        let repo = tmp("late-capture");
        if !init_git_commit(&repo.0, &[("a.txt", "head\n")]) {
            return;
        }
        let cwd = repo.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("s1", &cwd).unwrap();
        std::fs::write(repo.0.join("a.txt"), "agent\n").unwrap();
        store.capture("s1", &cwd, &["a.txt".into()]).unwrap();
        store.undo("s1", &cwd, None).unwrap();
        assert_eq!(
            std::fs::read_to_string(repo.0.join("a.txt")).unwrap(),
            "head\n"
        );
    }

    #[test]
    fn sync_picks_up_new_files_without_capture() {
        let repo = tmp("sync-new");
        if !init_git_commit(&repo.0, &[("clean.txt", "head\n")]) {
            return;
        }
        let cwd = repo.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("s1", &cwd).unwrap();
        std::fs::write(repo.0.join("clean.txt"), "agent\n").unwrap();
        std::fs::write(repo.0.join("created.txt"), "new\n").unwrap();
        store.sync("s1", &cwd).unwrap();
        assert_eq!(
            relatives(&store.status("s1", &cwd).unwrap()),
            vec!["clean.txt", "created.txt"]
        );
    }

    #[test]
    fn sync_does_not_claim_preexisting_dirty_files() {
        let repo = tmp("sync-skip-dirty");
        if !init_git_commit(&repo.0, &[("dirty.txt", "head\n"), ("clean.txt", "head\n")]) {
            return;
        }
        std::fs::write(repo.0.join("dirty.txt"), "user\n").unwrap();
        let cwd = repo.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("s1", &cwd).unwrap();

        std::fs::write(repo.0.join("dirty.txt"), "someone-else\n").unwrap();
        std::fs::write(repo.0.join("clean.txt"), "agent\n").unwrap();
        store.sync("s1", &cwd).unwrap();

        assert_eq!(
            relatives(&store.status("s1", &cwd).unwrap()),
            vec!["clean.txt"]
        );
        store.undo("s1", &cwd, None).unwrap();
        assert_eq!(
            std::fs::read_to_string(repo.0.join("dirty.txt")).unwrap(),
            "someone-else\n"
        );
        assert_eq!(
            std::fs::read_to_string(repo.0.join("clean.txt")).unwrap(),
            "head\n"
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
        store.ensure("s1", &cwd).unwrap();

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
        store.ensure("s1", &cwd).unwrap();

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
        store.ensure("s1", &cwd).unwrap();

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
        store.ensure("s1", &cwd).unwrap();

        std::fs::write(repo.0.join("a.txt"), "a\nA\n").unwrap();
        record(&store, "s1", &cwd, &["a.txt"]);

        let stats = store.stats_for_sessions(&cwd, &["s1".into()]).unwrap();
        let s1 = stats.get("s1").expect("s1 stats");
        assert_eq!(s1.files, 1);
        assert_eq!(s1.additions, 1);
        assert_eq!(s1.deletions, 0);
        assert_eq!(relatives(&store.status("s1", &cwd).unwrap()), vec!["a.txt"]);
    }

    fn counts(before: &str, after: &str) -> (i64, i64) {
        count_line_changes(
            &split_lines(before.as_bytes()),
            &split_lines(after.as_bytes()),
        )
    }

    #[test]
    fn line_counts_ignore_untouched_head_and_tail() {
        assert_eq!(counts("a\nb\nc\n", "a\nB\nc\n"), (1, 1));
        assert_eq!(counts("a\nc\n", "a\nb\nc\n"), (1, 0));
        assert_eq!(counts("a\nb\nc\n", "a\nc\n"), (0, 1));
        assert_eq!(counts("a\nb\n", "a\nb\n"), (0, 0));
    }

    #[test]
    fn line_counts_handle_empty_sides() {
        assert_eq!(counts("", "a\nb\n"), (2, 0));
        assert_eq!(counts("a\nb\n", ""), (0, 2));
        assert_eq!(counts("", ""), (0, 0));
    }

    #[test]
    fn line_counts_reuse_moved_lines() {
        // A block inserted in the middle must not bill the lines after it.
        assert_eq!(counts("a\nb\nc\nd\n", "a\nx\ny\nb\nc\nd\n"), (2, 0));
    }

    #[test]
    fn trailing_newline_is_not_a_line() {
        assert_eq!(split_lines(b"a\nb\n").len(), 2);
        assert_eq!(split_lines(b"a\nb").len(), 2);
        assert_eq!(split_lines(b"").len(), 0);
    }

    #[test]
    fn binary_and_skipped_sides_report_nothing() {
        let binary = FileState::Contents(vec![b'a', 0, b'b']);
        let text = FileState::Contents(b"a\n".to_vec());
        assert_eq!(line_diff_counts(&binary, &text), (0, 0));
        assert_eq!(line_diff_counts(&FileState::Skipped, &text), (0, 0));
        assert_eq!(line_diff_counts(&FileState::Missing, &text), (1, 0));
    }

    /// The pre-existing dirt must stay off the session's tab.
    #[test]
    fn counts_exclude_edits_made_before_the_session() {
        let repo = tmp("counts-baseline");
        if !init_git_commit(&repo.0, &[("a.txt", "one\n")]) {
            return;
        }
        // Someone edits the file by hand, then the session starts and adds a line.
        std::fs::write(repo.0.join("a.txt"), "one\nuser\n").unwrap();
        let cwd = repo.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("s1", &cwd).unwrap();

        std::fs::write(repo.0.join("a.txt"), "one\nuser\nagent\n").unwrap();
        record(&store, "s1", &cwd, &["a.txt"]);

        let status = store.status("s1", &cwd).unwrap();
        let file = status.files.first().expect("a.txt");
        assert_eq!((file.additions, file.deletions), (1, 0));
    }

    #[test]
    fn file_diff_bases_on_the_session_snapshot() {
        let repo = tmp("file-diff-base");
        if !init_git_commit(&repo.0, &[("a.txt", "one\n")]) {
            return;
        }
        std::fs::write(repo.0.join("a.txt"), "one\nuser\n").unwrap();
        let cwd = repo.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("s1", &cwd).unwrap();
        std::fs::write(repo.0.join("a.txt"), "one\nuser\nagent\n").unwrap();
        record(&store, "s1", &cwd, &["a.txt"]);

        let diff = store.file_diff("s1", &cwd, "a.txt").unwrap();
        assert_eq!(diff.original, "one\nuser\n");
        assert_eq!(diff.current, "one\nuser\nagent\n");
    }

    /// Files that were clean at session start have no snapshot, so HEAD is the base.
    #[test]
    fn file_diff_falls_back_to_head_without_a_snapshot() {
        let repo = tmp("file-diff-head");
        if !init_git_commit(&repo.0, &[("a.txt", "one\n")]) {
            return;
        }
        let cwd = repo.0.to_string_lossy().into_owned();
        let (_root, store) = store();
        store.ensure("s1", &cwd).unwrap();
        std::fs::write(repo.0.join("a.txt"), "one\ntwo\n").unwrap();
        record(&store, "s1", &cwd, &["a.txt"]);

        let diff = store.file_diff("s1", &cwd, "a.txt").unwrap();
        assert_eq!(diff.original, "one\n");
        assert_eq!(diff.current, "one\ntwo\n");
    }

    #[test]
    fn rejects_invalid_session_id() {
        let err = validate_id("../x", "session").unwrap_err();
        assert!(err.contains("Invalid"));
    }
}
