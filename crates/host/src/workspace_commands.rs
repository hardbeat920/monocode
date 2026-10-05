//! Port of host/workspace-commands.ts: the desktop's file and Git commands,
//! answered by the host for a remote project, with the same names and
//! arguments as the app's Tauri commands, so the same UI works on either
//! machine. Paths are absolute host paths and must lie inside a registered
//! project or one of its worktrees.
//!
//! Each command checks the path, then calls the same `monocode_git`
//! function the local app uses, within the host's response bounds.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine as _;
use monocode_remote::host::HostStore;
use parking_lot::Mutex;
use serde_json::{Map, Value, json};

use crate::browse::lexical_resolve;
use crate::git_branches::{create_host_branch, host_branches, switch_host_branch};
use crate::git_worktrees::host_worktrees;
use crate::skills::list_host_skills;
use crate::workspace::{
    MAX_FILE, create_host_path, existing_path, host_file_diff, host_git_action, host_git_index,
    index_host_files, list_host_files, search_host_content, slash_relative, workspace_path,
};

/// `WORKSPACE_COMMANDS`.
pub const WORKSPACE_COMMANDS: [&str; 42] = [
    "list_dir",
    "list_project_files",
    "read_text_file",
    "read_binary_file",
    "read_file_preview",
    "write_text_file",
    "stat_files",
    "create_path",
    "rename_path",
    "delete_path",
    "copy_path",
    "move_path",
    "git_diff_index",
    "git_diff_files",
    "git_diff_stats",
    "git_file_diff",
    "git_stage_contents",
    "git_stage_file",
    "git_unstage_file",
    "git_discard_file",
    "git_discard_all",
    "git_stage_all",
    "git_unstage_all",
    "git_commit",
    "git_head_message",
    "git_push",
    "git_pull",
    "git_sync",
    "git_pr_status",
    "git_pr_create",
    "git_history",
    "git_commit_files",
    "git_commit_file_diff",
    "git_staged_context",
    "git_range_context",
    "git_branches",
    "git_checkout",
    "git_create_branch",
    "git_stash",
    "git_worktrees",
    "search_project",
    "list_skills",
];

// Remote RPC has bounded request and response bodies. Keep file operations
// within those bounds even after JSON escaping or base64 encoding.
const MAX_TEXT_FILE: u64 = MAX_FILE;
const MAX_PREVIEW_FILE: u64 = 10 * 1024 * 1024;
const MAX_STAT_FILES: usize = 64;
const ROOTS_TTL: Duration = Duration::from_secs(5);

fn slashed(path: &str) -> String {
    path.replace('\\', "/")
}

/// `joined`: `parent/name` with forward slashes.
fn joined(parent: &str, name: &str) -> String {
    let parent = slashed(parent);
    let name = slashed(name);
    format!(
        "{}/{}",
        parent.trim_end_matches('/'),
        name.trim_matches('/')
    )
}

fn already_exists(name: &str) -> String {
    format!(
        "A file or folder {name} already exists at this location. Please choose a different name."
    )
}

fn to_json(value: impl serde::Serialize) -> Result<Option<Value>, String> {
    serde_json::to_value(value)
        .map(Some)
        .map_err(|error| error.to_string())
}

/// Where a path lies: the allowed root that contains it, and the part
/// below that root.
struct Located {
    root: PathBuf,
    relative: String,
}

/// Runs `action` while the project is idle, through the engine.
pub type IdleProject<'a> =
    &'a dyn Fn(&str, bool, &mut dyn FnMut() -> Result<Value, String>) -> Result<Value, String>;

struct CachedRoots {
    at: Instant,
    roots: Vec<String>,
}

/// `WorkspaceCommands`.
pub struct WorkspaceCommands {
    store: Arc<HostStore>,
    roots: Mutex<HashMap<String, CachedRoots>>,
    generation: Mutex<u64>,
}

impl WorkspaceCommands {
    pub fn new(store: Arc<HostStore>) -> Self {
        Self {
            store,
            roots: Mutex::new(HashMap::new()),
            generation: Mutex::new(0),
        }
    }

    /// `invalidateRoots`, after a worktree is created.
    pub fn invalidate_roots(&self) {
        *self.generation.lock() += 1;
        self.roots.lock().clear();
    }

    /// `run`. `None` answers `null`.
    pub fn run(
        &self,
        command: Option<&Value>,
        args: Option<&Value>,
        idle: IdleProject<'_>,
    ) -> Result<Option<Value>, String> {
        let command = command
            .and_then(Value::as_str)
            .filter(|command| WORKSPACE_COMMANDS.contains(command))
            .ok_or("Unsupported workspace command")?;
        let empty = Map::new();
        let input = args.and_then(Value::as_object).unwrap_or(&empty);
        let get = |key: &str| input.get(key);
        let force = || get("force") == Some(&Value::Bool(true));
        match command {
            "list_dir" => self.list_dir(get("path")),
            "list_project_files" => self.list_project_files(get("cwd")),
            "read_text_file" => self.read_text(get("path")),
            "read_binary_file" => self.read_binary(get("path")),
            "read_file_preview" => self.preview(get("path"), get("maxLines"), get("startLine")),
            "write_text_file" => self.write_text(get("path"), get("content")),
            "stat_files" => self.stat_files(get("paths")),
            "create_path" => self.create(get("parent"), get("name"), get("isDir")),
            "rename_path" => self.rename(get("path"), get("name")),
            "delete_path" => self.delete(get("path")),
            "copy_path" => self.copy(get("from"), get("destParent")),
            "move_path" => self.move_to(get("from"), get("destParent")),
            "git_diff_index" | "git_diff_files" => {
                to_json(host_git_index(&self.git_root(get("cwd"))?)?)
            }
            "git_diff_stats" => {
                let index = host_git_index(&self.git_root(get("cwd"))?)?;
                to_json(json!({
                    "files": index.files.len(),
                    "additions": index.additions,
                    "deletions": index.deletions,
                }))
            }
            "git_file_diff" => to_json(host_file_diff(
                &self.git_root(get("cwd"))?,
                get("relative"),
                get("staged") == Some(&Value::Bool(true)),
            )?),
            "git_stage_contents" => self.git_action(
                get("cwd"),
                "stageContents",
                get("relative"),
                None,
                get("contents"),
            ),
            "git_stage_file" => self.git_action(get("cwd"), "stage", get("relative"), None, None),
            "git_unstage_file" => {
                self.git_action(get("cwd"), "unstage", get("relative"), None, None)
            }
            "git_discard_file" => {
                self.git_action(get("cwd"), "discard", get("relative"), None, None)
            }
            "git_discard_all" => self.git_action(get("cwd"), "discardAll", None, None, None),
            "git_stage_all" => self.git_action(get("cwd"), "stageAll", None, None, None),
            "git_unstage_all" => self.git_action(get("cwd"), "unstageAll", None, None, None),
            "git_commit" => self.git_commit(get("cwd"), get("message"), get("amend")),
            "git_head_message" => to_json(monocode_git::fs::git_head_message(
                self.git_cwd(get("cwd"))?,
            )?),
            "git_push" => self.git_action(get("cwd"), "push", None, None, None),
            "git_pull" => {
                monocode_git::fs::git_pull(self.git_cwd(get("cwd"))?)?;
                Ok(None)
            }
            "git_sync" => {
                monocode_git::fs::git_sync(self.git_cwd(get("cwd"))?)?;
                Ok(None)
            }
            "git_pr_status" => to_json(monocode_git::fs::git_pr_status(self.git_cwd(get("cwd"))?)?),
            "git_pr_create" => self.git_pr_create(
                get("cwd"),
                get("title"),
                get("body"),
                get("base"),
                get("head"),
            ),
            "git_history" => self.git_history(get("cwd"), get("limit")),
            "git_commit_files" => {
                let cwd = self.git_cwd(get("cwd"))?;
                to_json(monocode_git::fs::git_commit_files(
                    cwd,
                    git_sha(get("sha"))?,
                )?)
            }
            "git_commit_file_diff" => {
                self.git_commit_file_diff(get("cwd"), get("sha"), get("relative"))
            }
            "git_staged_context" => to_json(monocode_git::fs::git_staged_context(
                self.git_cwd(get("cwd"))?,
            )?),
            "git_range_context" => to_json(monocode_git::fs::git_range_context(
                self.git_cwd(get("cwd"))?,
            )?),
            "git_branches" => self.git_branches(get("cwd")),
            "list_skills" => self.list_skills(get("cwd"), get("disabledPaths")),
            "git_checkout" => {
                let root = self.git_cwd(get("cwd"))?;
                let (name, remote) = (get("name"), get("remote"));
                let state = self.with_idle_git_project(&root, force(), idle, &mut || {
                    serde_json::to_value(switch_host_branch(&root, name, remote)?)
                        .map_err(|error| error.to_string())
                })?;
                to_json(state["current"].as_str().unwrap_or("HEAD"))
            }
            "git_create_branch" => {
                let root = self.git_cwd(get("cwd"))?;
                let name = get("name");
                let state = self.with_idle_git_project(&root, force(), idle, &mut || {
                    serde_json::to_value(create_host_branch(&root, name)?)
                        .map_err(|error| error.to_string())
                })?;
                to_json(state["current"].as_str().unwrap_or("HEAD"))
            }
            "git_stash" => self.git_stash(get("cwd"), get("message")),
            "git_worktrees" => self.git_worktrees(get("cwd")),
            "search_project" => self.search_project(get("options")),
            _ => Err("Unsupported workspace command".into()),
        }
    }

    /// `allowedRoots`: the project folders and worktrees files may be read
    /// and written in.
    fn allowed_roots(&self) -> Result<Vec<String>, String> {
        let generation = *self.generation.lock();
        let mut out = Vec::new();
        for project in self.store.projects()? {
            if let Some(cached) = self.roots.lock().get(&project.cwd)
                && cached.at.elapsed() < ROOTS_TTL
            {
                out.extend(cached.roots.iter().cloned());
                continue;
            }
            let roots = match host_worktrees(&project.cwd) {
                Ok(listed) => std::iter::once(project.cwd.clone())
                    .chain(
                        listed
                            .worktrees
                            .into_iter()
                            .filter(|tree| !tree.missing && tree.path != project.cwd)
                            .map(|tree| tree.path),
                    )
                    .collect(),
                Err(_) => vec![project.cwd.clone()],
            };
            if generation == *self.generation.lock() {
                self.roots.lock().insert(
                    project.cwd.clone(),
                    CachedRoots {
                        at: Instant::now(),
                        roots: roots.clone(),
                    },
                );
            }
            out.extend(roots);
        }
        Ok(out)
    }

    /// `locate`: the project root that contains `input`, which may not exist
    /// yet.
    fn locate(&self, input: Option<&Value>) -> Result<Located, String> {
        let input = match input {
            Some(Value::String(input))
                if Path::new(input).is_absolute()
                    && monocode_core::js::len(input) <= 4096
                    && !input.contains('\0') =>
            {
                input
            }
            _ => return Err("Invalid workspace path".into()),
        };
        let mut actual = lexical_resolve(Path::new(input));
        let mut missing: Vec<String> = Vec::new();
        // Resolve symlinks on the nearest existing ancestor, then re-append
        // the part that does not exist yet.
        loop {
            if let Ok(real) = dunce::canonicalize(&actual) {
                actual = missing
                    .iter()
                    .rev()
                    .fold(real, |path, part| path.join(part));
                break;
            }
            let Some(parent) = actual.parent().map(Path::to_path_buf) else {
                break;
            };
            if let Some(name) = actual.file_name() {
                missing.push(name.to_string_lossy().into_owned());
            }
            actual = parent;
        }
        for root in self.allowed_roots()? {
            let root = dunce::simplified(Path::new(&root)).to_path_buf();
            if actual.starts_with(&root) {
                return Ok(Located {
                    relative: slash_relative(&root, &actual),
                    root,
                });
            }
        }
        Err("Path is outside this machine’s projects".into())
    }

    fn existing(
        &self,
        input: Option<&Value>,
        allow_root: bool,
    ) -> Result<(PathBuf, PathBuf), String> {
        let located = self.locate(input)?;
        let path = existing_path(
            &located.root,
            Some(&Value::String(located.relative)),
            allow_root,
        )?;
        Ok((located.root, path))
    }

    fn list_dir(&self, input: Option<&Value>) -> Result<Option<Value>, String> {
        let located = self.locate(input)?;
        let parent = input.and_then(Value::as_str).unwrap_or_default();
        let entries = list_host_files(&located.root, Some(&Value::String(located.relative)))?;
        to_json(
            entries
                .into_iter()
                .map(|mut entry| {
                    entry.path = joined(parent, &entry.name);
                    entry
                })
                .collect::<Vec<_>>(),
        )
    }

    fn list_project_files(&self, input: Option<&Value>) -> Result<Option<Value>, String> {
        let (_, path) = self.existing(input, true)?;
        let cwd = input.and_then(Value::as_str).unwrap_or_default();
        to_json(
            index_host_files(&path)?
                .into_iter()
                .map(|file| {
                    json!({
                        "name": file.rsplit('/').next().filter(|name| !name.is_empty()).unwrap_or(&file),
                        "path": joined(cwd, &file),
                        "relative": file,
                    })
                })
                .collect::<Vec<_>>(),
        )
    }

    /// `file`: an existing file no larger than `limit`.
    fn file(&self, input: Option<&Value>, limit: u64, too_large: &str) -> Result<PathBuf, String> {
        let (_, path) = self.existing(input, false)?;
        let meta = std::fs::metadata(&path).map_err(|error| error.to_string())?;
        if !meta.is_file() {
            return Err("Not a file".into());
        }
        if meta.len() > limit {
            return Err(format!(
                "File is too large to {too_large} (maximum {} MB).",
                limit / 1024 / 1024
            ));
        }
        Ok(path)
    }

    fn read_text(&self, input: Option<&Value>) -> Result<Option<Value>, String> {
        let path = self.file(input, MAX_TEXT_FILE, "edit")?;
        to_json(monocode_git::fs::read_text_file(
            path.to_string_lossy().into_owned(),
        )?)
    }

    /// Base64, since host responses are JSON; the client decodes it.
    fn read_binary(&self, input: Option<&Value>) -> Result<Option<Value>, String> {
        let path = self.file(input, MAX_PREVIEW_FILE, "preview")?;
        let bytes = monocode_git::fs::read_binary_file(path.to_string_lossy().into_owned())?;
        to_json(base64::engine::general_purpose::STANDARD.encode(bytes))
    }

    fn preview(
        &self,
        input: Option<&Value>,
        max_lines: Option<&Value>,
        start_line: Option<&Value>,
    ) -> Result<Option<Value>, String> {
        use monocode_remote::host::js;
        let path = self.file(input, MAX_TEXT_FILE, "preview")?;
        let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
        let text = String::from_utf8_lossy(&bytes);
        if text.contains('\0') {
            return Err("Binary file".into());
        }
        let or = |value: Option<&Value>, fallback: f64| {
            let number = js::number(value);
            if number.is_nan() || number == 0.0 {
                fallback
            } else {
                number
            }
        };
        let limit = or(max_lines, 6.0).clamp(1.0, 12.0).trunc() as usize;
        // `slice` truncates fractional bounds, as the casts do.
        let start = or(start_line, 1.0).max(1.0).min(usize::MAX as f64) as usize;
        let lines: Vec<String> = text
            .split('\n')
            .map(|line| line.strip_suffix('\r').unwrap_or(line))
            .skip(start - 1)
            .take(limit)
            .map(|line| {
                if monocode_core::js::len(line) > 200 {
                    format!("{}…", monocode_core::js::slice_prefix(line, 199))
                } else {
                    line.to_string()
                }
            })
            .collect();
        to_json(lines)
    }

    fn write_text(
        &self,
        input: Option<&Value>,
        content: Option<&Value>,
    ) -> Result<Option<Value>, String> {
        let Some(Value::String(content)) = content else {
            return Err("Invalid file content".into());
        };
        if content.len() as u64 > MAX_TEXT_FILE {
            return Err(format!(
                "File is too large to save (maximum {} MB).",
                MAX_TEXT_FILE / 1024 / 1024
            ));
        }
        let located = self.locate(input)?;
        let path = workspace_path(&located.root, Some(&Value::String(located.relative)), false)?;
        if path.is_dir() {
            return Err("Cannot save text to a directory.".into());
        }
        // The local command replaces the file atomically, as the TypeScript did.
        monocode_git::fs::write_text_file(path.to_string_lossy().into_owned(), content.clone())?;
        Ok(None)
    }

    fn stat_files(&self, input: Option<&Value>) -> Result<Option<Value>, String> {
        let paths = input
            .and_then(Value::as_array)
            .filter(|paths| paths.len() <= MAX_STAT_FILES)
            .ok_or("Too many paths")?;
        to_json(
            paths
                .iter()
                .map(|path| {
                    let mtime = self
                        .existing(Some(path), false)
                        .ok()
                        .and_then(|(_, actual)| std::fs::metadata(actual).ok())
                        .filter(|meta| meta.is_file())
                        .and_then(|meta| meta.modified().ok())
                        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|elapsed| elapsed.as_millis() as i64);
                    json!({
                        "path": monocode_remote::host::js::string(Some(path)),
                        "mtimeMs": mtime,
                    })
                })
                .collect::<Vec<_>>(),
        )
    }

    fn create(
        &self,
        parent: Option<&Value>,
        name: Option<&Value>,
        is_dir: Option<&Value>,
    ) -> Result<Option<Value>, String> {
        let located = self.locate(parent)?;
        if let Err(error) = create_host_path(
            &located.root,
            Some(&Value::String(located.relative)),
            name,
            is_dir,
        ) {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                return Err(already_exists(&monocode_remote::host::js::string(name)));
            }
            return Err(error.to_string());
        }
        to_json(joined(
            parent.and_then(Value::as_str).unwrap_or_default(),
            name.and_then(Value::as_str).unwrap_or_default(),
        ))
    }

    fn rename(&self, input: Option<&Value>, name: Option<&Value>) -> Result<Option<Value>, String> {
        let name = match name {
            Some(Value::String(name))
                if !monocode_core::js::trim(name).is_empty()
                    && !name.starts_with(['/', '\\'])
                    && !name.contains('\0') =>
            {
                name
            }
            _ => return Err("A file or folder name must be provided.".into()),
        };
        let (root, from) = self.existing(input, false)?;
        let original = input.and_then(Value::as_str).unwrap_or_default();
        let parent = Path::new(original)
            .parent()
            .map(|parent| parent.to_string_lossy().into_owned())
            .unwrap_or_default();
        let target = joined(&parent, name);
        let located = self.locate(Some(&Value::String(target.clone())))?;
        if located.root != root {
            return Err("Cannot move a file between working copies.".into());
        }
        let to = workspace_path(&root, Some(&Value::String(located.relative)), false)?;
        if to == from {
            return to_json(original);
        }
        if std::fs::symlink_metadata(&to).is_ok() {
            return Err(already_exists(name));
        }
        if to.starts_with(&from) {
            return Err("Cannot move a folder into itself.".into());
        }
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        std::fs::rename(&from, &to).map_err(|error| error.to_string())?;
        to_json(target)
    }

    fn delete(&self, input: Option<&Value>) -> Result<Option<Value>, String> {
        let (_, path) = self.existing(input, false)?;
        monocode_git::fs::delete_path(path.to_string_lossy().into_owned())?;
        Ok(None)
    }

    /// `destination`: checks a paste or move into `dest_parent`.
    fn destination(
        &self,
        from: Option<&Value>,
        dest_parent: Option<&Value>,
    ) -> Result<(PathBuf, PathBuf), String> {
        let (_, source) = self.existing(from, false)?;
        let (_, parent) = self.existing(dest_parent, true)?;
        if !parent.is_dir() {
            return Err(format!(
                "{} is not a folder",
                monocode_remote::host::js::string(dest_parent)
            ));
        }
        if source.is_dir() && parent.starts_with(&source) {
            return Err("Cannot paste a folder into itself.".into());
        }
        Ok((source, parent))
    }

    fn copy(
        &self,
        from: Option<&Value>,
        dest_parent: Option<&Value>,
    ) -> Result<Option<Value>, String> {
        let (source, parent) = self.destination(from, dest_parent)?;
        let copied = monocode_git::fs::copy_path(
            source.to_string_lossy().into_owned(),
            parent.to_string_lossy().into_owned(),
        )?;
        let name = Path::new(&copied)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        to_json(joined(
            dest_parent.and_then(Value::as_str).unwrap_or_default(),
            &name,
        ))
    }

    fn move_to(
        &self,
        from: Option<&Value>,
        dest_parent: Option<&Value>,
    ) -> Result<Option<Value>, String> {
        let (source, parent) = self.destination(from, dest_parent)?;
        let name = source
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let to = parent.join(&name);
        if to == source {
            return to_json(from.and_then(Value::as_str).unwrap_or_default());
        }
        if std::fs::symlink_metadata(&to).is_ok() {
            return Err(already_exists(&name));
        }
        std::fs::rename(&source, &to).map_err(|error| error.to_string())?;
        to_json(joined(
            dest_parent.and_then(Value::as_str).unwrap_or_default(),
            &name,
        ))
    }

    /// `listSkills`: the skills this machine's agents see in a project.
    fn list_skills(
        &self,
        cwd: Option<&Value>,
        disabled: Option<&Value>,
    ) -> Result<Option<Value>, String> {
        let (_, path) = self.existing(cwd, true)?;
        let disabled: Vec<String> = disabled
            .and_then(Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let home = monocode_remote::host::server::home_dir();
        let skills = list_host_skills(&path, Some(&home), Some(&disabled));
        to_json(
            skills
                .into_iter()
                .map(|mut skill| {
                    skill.path = slashed(&skill.path);
                    skill
                })
                .collect::<Vec<_>>(),
        )
    }

    /// `gitRoot`.
    fn git_root(&self, input: Option<&Value>) -> Result<PathBuf, String> {
        let (_, path) = self.existing(input, true)?;
        if !path.is_dir() {
            return Err("Not a working copy".into());
        }
        Ok(path)
    }

    fn git_cwd(&self, input: Option<&Value>) -> Result<String, String> {
        Ok(self.git_root(input)?.to_string_lossy().into_owned())
    }

    fn search_project(&self, input: Option<&Value>) -> Result<Option<Value>, String> {
        let options = input.and_then(Value::as_object).ok_or("Invalid search")?;
        let root = self.git_root(options.get("cwd"))?;
        search_host_content(&root, options).map(Some)
    }

    fn git_action(
        &self,
        cwd: Option<&Value>,
        action: &str,
        relative: Option<&Value>,
        message: Option<&Value>,
        contents: Option<&Value>,
    ) -> Result<Option<Value>, String> {
        let root = self.git_root(cwd)?;
        host_git_action(
            &root,
            Some(&Value::String(action.into())),
            relative,
            message,
            contents,
        )
    }

    fn git_commit(
        &self,
        cwd: Option<&Value>,
        message: Option<&Value>,
        amend: Option<&Value>,
    ) -> Result<Option<Value>, String> {
        let message = match message {
            Some(Value::String(message))
                if !monocode_core::js::trim(message).is_empty()
                    && monocode_core::js::len(message) <= 100_000 =>
            {
                message.clone()
            }
            _ => return Err("Enter a commit message".into()),
        };
        monocode_git::fs::git_commit(
            self.git_cwd(cwd)?,
            message,
            amend == Some(&Value::Bool(true)),
        )?;
        Ok(None)
    }

    fn git_pr_create(
        &self,
        cwd: Option<&Value>,
        title: Option<&Value>,
        body: Option<&Value>,
        base: Option<&Value>,
        head: Option<&Value>,
    ) -> Result<Option<Value>, String> {
        let text = |value: Option<&Value>| match value {
            Some(Value::String(text)) if monocode_core::js::len(text) <= 100_000 => {
                Ok(text.clone())
            }
            _ => Err("Invalid pull request".to_string()),
        };
        let (title, body, base, head) = (text(title)?, text(body)?, text(base)?, text(head)?);
        to_json(monocode_git::fs::git_pr_create(
            self.git_cwd(cwd)?,
            title,
            body,
            base,
            head,
        )?)
    }

    fn git_history(
        &self,
        cwd: Option<&Value>,
        limit: Option<&Value>,
    ) -> Result<Option<Value>, String> {
        let count = monocode_remote::host::js::safe_integer(limit)
            .map(|limit| limit.clamp(1, 500))
            .unwrap_or(200);
        to_json(monocode_git::fs::git_history(
            self.git_cwd(cwd)?,
            Some(count as u32),
        )?)
    }

    fn git_commit_file_diff(
        &self,
        cwd: Option<&Value>,
        sha: Option<&Value>,
        relative: Option<&Value>,
    ) -> Result<Option<Value>, String> {
        let sha = git_sha(sha)?;
        let root = self.git_root(cwd)?;
        let path = slash_relative(&root, &workspace_path(&root, relative, false)?);
        let mut diff = monocode_git::fs::git_commit_file_diff(
            root.to_string_lossy().into_owned(),
            sha,
            path.clone(),
        )?;
        if !diff.too_large
            && (diff.original.len() as u64 > MAX_TEXT_FILE
                || diff.current.len() as u64 > MAX_TEXT_FILE)
        {
            diff.too_large = true;
            diff.original.clear();
            diff.current.clear();
        }
        diff.path = path.clone();
        diff.relative = path;
        to_json(diff)
    }

    fn git_branches(&self, cwd: Option<&Value>) -> Result<Option<Value>, String> {
        let state = host_branches(&self.git_cwd(cwd)?)?;
        let current = state.current.clone();
        let mut branches: Vec<Value> = state
            .branches
            .iter()
            .map(|name| json!({ "name": name, "current": Some(name) == current.as_ref(), "remote": null }))
            .collect();
        branches.extend(
            state.remotes.iter().map(
                |entry| json!({ "name": entry.name, "current": false, "remote": entry.remote }),
            ),
        );
        to_json(json!({
            "current": current,
            "detached": current.is_none(),
            "branches": branches,
        }))
    }

    /// `withIdleGitProject`: the project that owns `cwd` must be idle.
    fn with_idle_git_project(
        &self,
        cwd: &str,
        force: bool,
        idle: IdleProject<'_>,
        action: &mut dyn FnMut() -> Result<Value, String>,
    ) -> Result<Value, String> {
        let root = self.locate(Some(&Value::String(cwd.into())))?.root;
        let root = root.to_string_lossy().into_owned();
        let project = self
            .store
            .projects()?
            .into_iter()
            .find(|candidate| {
                candidate.cwd == root
                    || self
                        .roots
                        .lock()
                        .get(&candidate.cwd)
                        .is_some_and(|cached| cached.roots.contains(&root))
            })
            .ok_or("Project is unavailable")?;
        idle(&project.id, force, action)
    }

    fn git_stash(
        &self,
        cwd: Option<&Value>,
        message: Option<&Value>,
    ) -> Result<Option<Value>, String> {
        let message = match message {
            None | Some(Value::Null) => None,
            Some(Value::String(message)) if monocode_core::js::len(message) <= 1000 => {
                Some(message.clone()).filter(|message| !message.is_empty())
            }
            Some(_) => return Err("Invalid stash message".into()),
        };
        monocode_git::fs::git_stash(self.git_cwd(cwd)?, message)?;
        Ok(None)
    }

    fn git_worktrees(&self, cwd: Option<&Value>) -> Result<Option<Value>, String> {
        let listed = host_worktrees(&self.git_cwd(cwd)?)?;
        to_json(json!({
            "defaultRoot": listed.default_root,
            "worktrees": listed.worktrees.iter().map(|tree| json!({
                "path": tree.path,
                "branch": tree.branch,
                "head": tree.head,
                "isMain": tree.is_main,
                "missing": tree.missing,
                "locked": false,
                "prunable": tree.missing,
                "dirty": null,
                "unpushed": null,
                "sessionIds": [],
            })).collect::<Vec<_>>(),
        }))
    }
}

/// `gitSha`.
fn git_sha(value: Option<&Value>) -> Result<String, String> {
    match value {
        Some(Value::String(sha))
            if (4..=40).contains(&sha.len()) && sha.chars().all(|c| c.is_ascii_hexdigit()) =>
        {
            Ok(sha.clone())
        }
        _ => Err("Invalid commit".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git_branches::tests::run_git;

    fn setup() -> (tempfile::TempDir, Arc<HostStore>, WorkspaceCommands, String) {
        let directory = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(directory.path()).unwrap();
        let project = root.join("project");
        std::fs::create_dir(&project).unwrap();
        let store = Arc::new(HostStore::open(&root.join("host.db")).unwrap());
        let cwd = project.to_string_lossy().into_owned();
        store.add_project(&cwd, "project").unwrap();
        let commands = WorkspaceCommands::new(store.clone());
        (directory, store, commands, cwd)
    }

    fn no_idle(
        _: &str,
        _: bool,
        action: &mut dyn FnMut() -> Result<Value, String>,
    ) -> Result<Value, String> {
        action()
    }

    fn run(
        commands: &WorkspaceCommands,
        command: &str,
        args: Value,
    ) -> Result<Option<Value>, String> {
        commands.run(Some(&json!(command)), Some(&args), &no_idle)
    }

    #[test]
    fn answers_file_commands_inside_projects_only() {
        let (_directory, _store, commands, cwd) = setup();
        let file = joined(&cwd, "src/app.ts");
        let outside = Path::new(&cwd).with_file_name("outside");
        assert_eq!(
            run(
                &commands,
                "create_path",
                json!({ "parent": cwd, "name": "src/app.ts", "isDir": false })
            )
            .unwrap(),
            Some(json!(file))
        );
        assert!(
            run(
                &commands,
                "create_path",
                json!({ "parent": cwd, "name": "src/app.ts", "isDir": false })
            )
            .unwrap_err()
            .contains("already exists")
        );
        run(
            &commands,
            "write_text_file",
            json!({ "path": file, "content": "one\ntwo\n" }),
        )
        .unwrap();
        assert_eq!(
            run(&commands, "read_text_file", json!({ "path": file })).unwrap(),
            Some(json!("one\ntwo\n"))
        );
        assert_eq!(
            run(
                &commands,
                "read_file_preview",
                json!({ "path": file, "maxLines": 1, "startLine": 2 })
            )
            .unwrap(),
            Some(json!(["two"]))
        );
        let listed = run(
            &commands,
            "list_dir",
            json!({ "path": joined(&cwd, "src") }),
        )
        .unwrap()
        .unwrap();
        assert_eq!(listed[0]["path"], json!(file));
        let stats = run(&commands, "stat_files", json!({ "paths": [file, outside] }))
            .unwrap()
            .unwrap();
        assert!(stats[0]["mtimeMs"].is_i64());
        assert_eq!(stats[1]["mtimeMs"], Value::Null);
        let renamed = run(
            &commands,
            "rename_path",
            json!({ "path": file, "name": "main.ts" }),
        )
        .unwrap()
        .unwrap();
        assert_eq!(renamed, json!(joined(&cwd, "src/main.ts")));
        let copied = run(
            &commands,
            "copy_path",
            json!({ "from": renamed, "destParent": cwd }),
        )
        .unwrap()
        .unwrap();
        assert_eq!(copied, json!(joined(&cwd, "main.ts")));
        run(&commands, "delete_path", json!({ "path": copied })).unwrap();
        assert!(
            run(&commands, "read_text_file", json!({ "path": outside }))
                .unwrap_err()
                .contains("outside this machine")
        );
        assert!(
            run(&commands, "read_text_file", json!({ "path": "relative" }))
                .unwrap_err()
                .contains("Invalid workspace path")
        );
        assert!(
            run(&commands, "no_such_command", json!({}))
                .unwrap_err()
                .contains("Unsupported workspace command")
        );
        assert_eq!(
            run(&commands, "list_project_files", json!({ "cwd": cwd }))
                .unwrap()
                .unwrap()[0]["relative"],
            json!("src/main.ts")
        );
    }

    #[test]
    fn answers_git_commands_and_branch_changes_through_the_idle_check() {
        let (_directory, _store, commands, cwd) = setup();
        let root = Path::new(&cwd);
        run_git(root, &["init", "-q"]);
        run_git(root, &["checkout", "-q", "-b", "main"]);
        std::fs::write(root.join("a.txt"), "a\n").unwrap();
        run_git(root, &["add", "a.txt"]);
        run_git(
            root,
            &[
                "-c",
                "user.name=T",
                "-c",
                "user.email=t@example.com",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-qm",
                "init",
            ],
        );
        let branches = run(&commands, "git_branches", json!({ "cwd": cwd }))
            .unwrap()
            .unwrap();
        assert_eq!(branches["current"], "main");
        assert_eq!(branches["detached"], false);
        let calls = std::cell::Cell::new(0);
        let counting = |_: &str, force: bool, action: &mut dyn FnMut() -> Result<Value, String>| {
            assert!(force);
            calls.set(calls.get() + 1);
            action()
        };
        let created = commands
            .run(
                Some(&json!("git_create_branch")),
                Some(&json!({ "cwd": cwd, "name": "feature", "force": true })),
                &counting,
            )
            .unwrap();
        assert_eq!(created, Some(json!("feature")));
        assert_eq!(calls.get(), 1);
        std::fs::write(root.join("a.txt"), "b\n").unwrap();
        let stats = run(&commands, "git_diff_stats", json!({ "cwd": cwd }))
            .unwrap()
            .unwrap();
        assert_eq!(stats, json!({ "files": 1, "additions": 1, "deletions": 1 }));
        let worktrees = run(&commands, "git_worktrees", json!({ "cwd": cwd }))
            .unwrap()
            .unwrap();
        assert_eq!(worktrees["worktrees"][0]["branch"], "feature");
        assert_eq!(worktrees["worktrees"][0]["sessionIds"], json!([]));
        let found = run(
            &commands,
            "search_project",
            json!({ "options": { "cwd": cwd, "query": "b" } }),
        )
        .unwrap()
        .unwrap();
        assert_eq!(found["matches"][0]["relative"], "a.txt");
        assert!(
            run(
                &commands,
                "git_commit_files",
                json!({ "cwd": cwd, "sha": "not-a-sha" })
            )
            .unwrap_err()
            .contains("Invalid commit")
        );
    }
}
