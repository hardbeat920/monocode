//! Port of host/workspace.ts: files and Git inside one host project root,
//! with paths relative to that root.
//!
//! The TypeScript duplicated the desktop's Tauri commands because Node could
//! not call Rust. Here the Git index, file diffs, staging, commits, pushes,
//! directory listings, and content search reuse `monocode_git`. What stays
//! host-specific is the path check that keeps every request inside the
//! root, the 1 MiB bounds that keep responses under the desktop's limit, and
//! the few functions whose local versions behave differently:
//!
//! - File listing for Go to File fails on a broken Git index instead of
//!   walking the tree, so ignored files never leak.
//! - Creating a path checks every folder it creates against the root, so a
//!   symlink inside the project cannot lead outside it.
//! - Saving compares the expected content first.

use std::path::{Component, Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use monocode_remote::host::exec::{ExecOptions, exec};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::browse::lexical_resolve;

/// The largest file the host reads, writes, or diffs as text.
pub const MAX_FILE: u64 = 1024 * 1024;

/// Upper bound on paths sent for Go to File, as for a local project.
const MAX_INDEXED_FILES: usize = 20_000;

fn outside() -> String {
    "Path is outside the workspace".into()
}

/// The part of `path` below `root`, when both are absolute and normalized.
fn below(root: &Path, path: &Path) -> Option<PathBuf> {
    dunce::simplified(path)
        .strip_prefix(dunce::simplified(root))
        .ok()
        .map(Path::to_path_buf)
}

fn has_git_component(relative: &Path) -> bool {
    relative.components().any(|part| match part {
        Component::Normal(name) => name.to_string_lossy().to_lowercase() == ".git",
        _ => false,
    })
}

/// `path.relative(root, path)` with `/` separators.
pub fn slash_relative(root: &Path, path: &Path) -> String {
    below(root, path)
        .map(|relative| {
            relative
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_default()
}

/// `workspacePath`: `input` resolved against `root`, refused when it leaves
/// the root, names `.git`, or is the root itself without `allow_root`.
pub fn workspace_path(
    root: &Path,
    input: Option<&Value>,
    allow_root: bool,
) -> Result<PathBuf, String> {
    let input = match input {
        Some(Value::String(input))
            if monocode_core::js::len(input) <= 4096 && !input.contains('\0') =>
        {
            input
        }
        _ => return Err("Invalid workspace path".into()),
    };
    let path = lexical_resolve(&root.join(input));
    let relative = below(root, &path).ok_or_else(outside)?;
    if (!allow_root && relative.as_os_str().is_empty()) || has_git_component(&relative) {
        return Err(outside());
    }
    Ok(path)
}

/// `existingPath`: [`workspace_path`], then the real path, checked again.
pub fn existing_path(
    root: &Path,
    input: Option<&Value>,
    allow_root: bool,
) -> Result<PathBuf, String> {
    let path = workspace_path(root, input, allow_root)?;
    let actual = dunce::canonicalize(&path).map_err(|error| error.to_string())?;
    let relative = below(root, &actual).ok_or_else(outside)?;
    if (!allow_root && relative.as_os_str().is_empty()) || has_git_component(&relative) {
        return Err(outside());
    }
    Ok(actual)
}

fn git(root: &Path, args: &[&str], max_buffer: usize) -> Result<String, String> {
    let mut full = vec!["-c", "core.pager=cat"];
    full.extend_from_slice(args);
    let mut env: Vec<_> = std::env::vars_os().collect();
    env.push(("LC_ALL".into(), "C".into()));
    exec(
        "git",
        &full,
        ExecOptions {
            cwd: Some(root.to_path_buf()),
            timeout: Duration::from_secs(10),
            max_buffer,
            env: Some(env),
            stdin: None,
        },
    )
    .map(|output| output.stdout)
    .map_err(|error| {
        if error.stderr.is_empty() {
            error.message
        } else {
            format!("{}\n{}", error.message.trim_end(), error.stderr.trim_end())
        }
    })
}

/// `HostFileEntry`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostFileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub ignored: bool,
}

/// `listHostFiles`: one folder's entries, with root-relative paths.
pub fn list_host_files(root: &Path, input: Option<&Value>) -> Result<Vec<HostFileEntry>, String> {
    let dir = existing_path(root, input, true)?;
    if !std::fs::metadata(&dir).is_ok_and(|meta| meta.is_dir()) {
        return Err("Path is not a directory".into());
    }
    if std::fs::read_dir(&dir)
        .map_err(|error| error.to_string())?
        .count()
        > 5000
    {
        return Err("Directory has too many entries to display".into());
    }
    let listed = serde_json::to_value(monocode_git::fs::list_dir_sync(&dir)?)
        .map_err(|error| error.to_string())?;
    let entries: Vec<HostFileEntry> =
        serde_json::from_value(listed).map_err(|error| error.to_string())?;
    Ok(entries
        .into_iter()
        .map(|mut entry| {
            let path = dir.join(&entry.name);
            let relative = slash_relative(root, &path);
            // A symlink counts as a folder only when it stays in the root.
            if std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_symlink()) {
                entry.is_dir = existing_path(root, Some(&Value::String(relative.clone())), false)
                    .is_ok_and(|actual| actual.is_dir());
            }
            entry.path = relative;
            entry
        })
        .collect())
}

/// `hostFilePaths`: every file in the worktree, honoring .gitignore in a
/// repository. A Git error other than "not a repository" fails the call.
fn host_file_paths(root: &Path) -> Result<Vec<String>, String> {
    let repository = match git(
        root,
        &["rev-parse", "--is-inside-work-tree"],
        4 * 1024 * 1024,
    ) {
        Ok(output) => output.trim() == "true",
        Err(error) if error.contains("not a git repository") => false,
        Err(error) => return Err(error),
    };
    if repository {
        return Ok(git(
            root,
            &[
                "ls-files",
                "-z",
                "--cached",
                "--others",
                "--exclude-standard",
            ],
            8 * 1024 * 1024,
        )?
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .collect());
    }
    let mut paths = Vec::new();
    let mut pending = vec![String::new()];
    while let Some(dir) = pending.pop() {
        if paths.len() >= 5000 {
            break;
        }
        let Ok(entries) = std::fs::read_dir(root.join(&dir)) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == ".git" || name == "node_modules" {
                continue;
            }
            let path = if dir.is_empty() {
                name
            } else {
                format!("{dir}/{name}")
            };
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => pending.push(path),
                Ok(kind) if kind.is_file() => paths.push(path),
                _ => {}
            }
        }
    }
    Ok(paths)
}

/// `indexHostFiles`: relative paths for fuzzy Go to File on the client.
pub fn index_host_files(root: &Path) -> Result<Vec<String>, String> {
    let mut paths = host_file_paths(root)?;
    paths.truncate(MAX_INDEXED_FILES);
    Ok(paths)
}

/// `searchHostFiles`: a bounded file name search.
pub fn search_host_files(root: &Path, input: Option<&Value>) -> Result<Vec<HostFileEntry>, String> {
    let query = match input {
        Some(Value::String(query)) if monocode_core::js::len(query) <= 200 => {
            monocode_core::js::trim(query).to_lowercase()
        }
        _ => return Err("Invalid search".into()),
    };
    if query.is_empty() {
        return Ok(Vec::new());
    }
    Ok(host_file_paths(root)?
        .into_iter()
        .filter(|path| path.to_lowercase().contains(&query))
        .take(200)
        .map(|path| HostFileEntry {
            name: path
                .rsplit('/')
                .next()
                .filter(|name| !name.is_empty())
                .unwrap_or(&path)
                .to_string(),
            path,
            is_dir: false,
            ignored: false,
        })
        .collect())
}

/// `searchOptions`: validates the request, then builds the shared search's
/// options.
fn search_options(
    root: &Path,
    input: &Map<String, Value>,
) -> Result<monocode_git::search::SearchOptions, String> {
    let invalid = || "Invalid search".to_string();
    let query = match input.get("query") {
        Some(Value::String(query))
            if monocode_core::js::len(query) <= 200 && !query.contains('\0') =>
        {
            query
        }
        _ => return Err(invalid()),
    };
    let glob = |key: &str| match input.get(key) {
        None => Ok(None),
        Some(Value::String(text))
            if monocode_core::js::len(text) <= 1000 && !text.contains('\0') =>
        {
            Ok(Some(text.clone()))
        }
        Some(_) => Err(invalid()),
    };
    let include = glob("include")?;
    let exclude = glob("exclude")?;
    let flag = |key: &str| match input.get(key) {
        None => Ok(false),
        Some(Value::Bool(flag)) => Ok(*flag),
        Some(_) => Err(invalid()),
    };
    Ok(monocode_git::search::SearchOptions {
        cwd: root.to_string_lossy().into_owned(),
        query: monocode_core::js::trim(query).to_string(),
        case_sensitive: flag("caseSensitive")?,
        whole_word: flag("wholeWord")?,
        regex: flag("regex")?,
        include,
        exclude,
        search_id: String::new(),
    })
}

/// `searchHostContent`: file contents, with the same controls and result
/// shape as a local project search.
pub fn search_host_content(root: &Path, input: &Map<String, Value>) -> Result<Value, String> {
    let options = search_options(root, input)?;
    if options.query.is_empty() {
        return Ok(json!({ "matches": [], "truncated": false }));
    }
    let result =
        monocode_git::search::search_project_sync(root, &options, &AtomicBool::new(false))?;
    serde_json::to_value(result).map_err(|error| error.to_string())
}

/// `readHostFile`: a text file of at most 1 MiB.
pub fn read_host_file(root: &Path, input: Option<&Value>) -> Result<String, String> {
    let path = existing_path(root, input, false)?;
    let meta = std::fs::metadata(&path).map_err(|error| error.to_string())?;
    if !meta.is_file() {
        return Err("Path is not a file".into());
    }
    if meta.len() > MAX_FILE {
        return Err("File is too large to preview".into());
    }
    let bytes = std::fs::read(&path).map_err(|error| error.to_string())?;
    if bytes.contains(&0) {
        return Err("Binary file cannot be previewed".into());
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// `writeHostFile`: saves only when the file still holds `expected`.
pub fn write_host_file(
    root: &Path,
    input: Option<&Value>,
    expected: Option<&Value>,
    content: Option<&Value>,
) -> Result<(), String> {
    let (Some(Value::String(expected)), Some(Value::String(content))) = (expected, content) else {
        return Err("Invalid file content".into());
    };
    if content.len() as u64 > MAX_FILE || content.contains('\0') {
        return Err("Invalid file content".into());
    }
    let path = existing_path(root, input, false)?;
    let current = read_host_file(root, input)?;
    if current != *expected {
        return Err("File changed on the host; reload before saving".into());
    }
    std::fs::write(path, content).map_err(|error| error.to_string())
}

fn invalid_name() -> String {
    "Invalid file name".into()
}

/// `createHostPath`: a new file or folder under an existing folder. Returns
/// the root-relative path.
pub fn create_host_path(
    root: &Path,
    parent: Option<&Value>,
    name: Option<&Value>,
    is_dir: Option<&Value>,
) -> Result<String, std::io::Error> {
    let invalid = |message: String| std::io::Error::other(message);
    let (Some(Value::String(name)), Some(Value::Bool(is_dir))) = (name, is_dir) else {
        return Err(invalid(invalid_name()));
    };
    if name.is_empty()
        || monocode_core::js::len(name) > 4096
        || name.contains('\0')
        || name.starts_with(['/', '\\'])
    {
        return Err(invalid(invalid_name()));
    }
    let parts: Vec<&str> = name
        .trim_end_matches(['/', '\\'])
        .split(['/', '\\'])
        .collect();
    if parts.is_empty()
        || parts.iter().any(|part| {
            part.is_empty()
                || *part == "."
                || *part == ".."
                || part.to_lowercase() == ".git"
                || monocode_core::js::len(part) > 255
                || part.chars().all(monocode_core::js::is_space)
        })
    {
        return Err(invalid(invalid_name()));
    }
    let relative_value = |path: &Path| Value::String(slash_relative(root, path));
    let mut directory = existing_path(root, parent, true).map_err(invalid)?;
    if !directory.is_dir() {
        return Err(invalid("Path is not a directory".into()));
    }
    for part in &parts[..parts.len() - 1] {
        let candidate = workspace_path(root, Some(&relative_value(&directory.join(part))), false)
            .map_err(invalid)?;
        if let Err(error) = std::fs::create_dir(&candidate)
            && error.kind() != std::io::ErrorKind::AlreadyExists
        {
            return Err(error);
        }
        directory =
            existing_path(root, Some(&relative_value(&candidate)), false).map_err(invalid)?;
        if !directory.is_dir() {
            return Err(invalid("Path is not a directory".into()));
        }
    }
    let last = parts[parts.len() - 1];
    let path = workspace_path(root, Some(&relative_value(&directory.join(last))), false)
        .map_err(invalid)?;
    if *is_dir {
        std::fs::create_dir(&path)?;
    } else {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
    }
    Ok(slash_relative(root, &path))
}

/// `hostGitIndex`: the changed files, with root-relative `path` values as
/// the TypeScript host sent them.
pub fn host_git_index(root: &Path) -> Result<monocode_git::fs::GitDiffIndex, String> {
    let mut index = monocode_git::fs::git_diff_index_for(root);
    for file in &mut index.files {
        file.path = file.relative.clone();
    }
    Ok(index)
}

/// `hostFileDiff`: original and current text of one changed file.
pub fn host_file_diff(
    root: &Path,
    input: Option<&Value>,
    staged: bool,
) -> Result<monocode_git::fs::GitFileDiff, String> {
    let path = workspace_path(root, input, false)?;
    let relative = slash_relative(root, &path);
    let index = host_git_index(root)?;
    let file = index
        .files
        .iter()
        .find(|entry| entry.relative == relative)
        .ok_or("File has no uncommitted changes")?;
    let mut diff = monocode_git::fs::git_file_diff(
        root.to_string_lossy().into_owned(),
        relative.clone(),
        staged,
    )?;
    // Keep responses under the desktop's limit, as the 1 MiB read did.
    if !diff.too_large
        && (diff.original.len() as u64 > MAX_FILE || diff.current.len() as u64 > MAX_FILE)
    {
        diff.too_large = true;
        diff.original.clear();
        diff.current.clear();
    }
    diff.path = relative.clone();
    diff.relative = relative;
    diff.status = file.status.clone();
    Ok(diff)
}

fn repo_path(root: &Path, input: Option<&Value>) -> Result<String, String> {
    Ok(slash_relative(root, &workspace_path(root, input, false)?))
}

/// `hostGitAction`. `None` answers `null`; `createPr` answers the pull
/// request URL.
pub fn host_git_action(
    root: &Path,
    action: Option<&Value>,
    input: Option<&Value>,
    message: Option<&Value>,
    contents: Option<&Value>,
) -> Result<Option<Value>, String> {
    let cwd = || root.to_string_lossy().into_owned();
    match action.and_then(Value::as_str) {
        Some("stageContents") => {
            let path = repo_path(root, input)?;
            let Some(Value::String(contents)) = contents else {
                return Err("File too large or invalid".into());
            };
            if contents.len() as u64 > MAX_FILE {
                return Err("File too large or invalid".into());
            }
            monocode_git::fs::git_stage_contents(cwd(), path, contents.clone())?;
        }
        Some("stage") => monocode_git::fs::git_stage_file(cwd(), repo_path(root, input)?)?,
        Some("unstage") => monocode_git::fs::git_unstage_file(cwd(), repo_path(root, input)?)?,
        Some("stageAll") => monocode_git::fs::git_stage_all(cwd())?,
        Some("unstageAll") => monocode_git::fs::git_unstage_all(cwd())?,
        Some("discard") => {
            let path = repo_path(root, input)?;
            if !host_git_index(root)?
                .files
                .iter()
                .any(|entry| entry.relative == path && entry.unstaged)
            {
                return Err("File has no changes to discard".into());
            }
            monocode_git::fs::git_discard_file(cwd(), path)?;
        }
        Some("discardAll") => monocode_git::fs::git_discard_all(cwd())?,
        Some("commit") => {
            let message = match message {
                Some(Value::String(message))
                    if !monocode_core::js::trim(message).is_empty()
                        && monocode_core::js::len(message) <= 100_000 =>
                {
                    message
                }
                _ => return Err("Enter a commit message".into()),
            };
            monocode_git::fs::git_commit(cwd(), message.clone(), false)?;
        }
        Some("push") => monocode_git::fs::git_push(cwd())?,
        Some("createPr") => {
            let mut env: Vec<_> = std::env::vars_os().collect();
            env.push(("GH_PROMPT_DISABLED".into(), "1".into()));
            env.push(("GIT_TERMINAL_PROMPT".into(), "0".into()));
            let output = exec(
                "gh",
                &["pr", "create", "--fill"],
                ExecOptions {
                    cwd: Some(root.to_path_buf()),
                    timeout: Duration::from_secs(30),
                    max_buffer: 1024 * 1024,
                    env: Some(env),
                    stdin: None,
                },
            )
            .map_err(String::from)?;
            return Ok(Some(Value::String(output.stdout.trim().to_string())));
        }
        _ => return Err("Unsupported Git action".into()),
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git_branches::tests::run_git;
    use serde_json::json;

    fn real_tempdir() -> (tempfile::TempDir, PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(directory.path()).unwrap();
        (directory, root)
    }

    fn options(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    fn init(root: &Path) {
        let git = |args: &[&str]| run_git(root, args);
        git(&["init", "-q"]);
        git(&["config", "core.autocrlf", "false"]);
        git(&["config", "user.name", "Workspace Test"]);
        git(&["config", "user.email", "workspace@example.test"]);
        git(&["config", "commit.gpgsign", "false"]);
    }

    /// workspace.test.ts: "reports a broken Git index instead of searching
    /// ignored files".
    #[test]
    fn reports_a_broken_git_index_instead_of_searching_ignored_files() {
        let (_directory, root) = real_tempdir();
        run_git(&root, &["init", "-q"]);
        std::fs::write(root.join(".gitignore"), "private.txt\n").unwrap();
        std::fs::write(root.join("private.txt"), "ignored").unwrap();
        std::fs::write(root.join(".git").join("index"), "broken").unwrap();
        assert!(search_host_files(&root, Some(&json!("private"))).is_err());
    }

    /// workspace.test.ts: "lists host files and rejects paths escaping the
    /// project".
    #[cfg(unix)]
    #[test]
    fn lists_host_files_and_rejects_paths_escaping_the_project() {
        let (_directory, root) = real_tempdir();
        std::fs::create_dir(root.join("src")).unwrap();
        std::fs::write(root.join("src").join("app.ts"), "source\n").unwrap();
        std::os::unix::fs::symlink(std::env::temp_dir(), root.join("outside")).unwrap();
        let listed = list_host_files(&root, Some(&json!(""))).unwrap();
        assert!(
            listed
                .iter()
                .any(|entry| entry.name == "src" && entry.is_dir)
        );
        let outside = listed.iter().find(|entry| entry.name == "outside").unwrap();
        assert!(!outside.is_dir);
        assert_eq!(
            search_host_files(&root, Some(&json!("app")))
                .unwrap()
                .into_iter()
                .map(|entry| entry.path)
                .collect::<Vec<_>>(),
            ["src/app.ts"]
        );
        assert_eq!(
            read_host_file(&root, Some(&json!("src/app.ts"))).unwrap(),
            "source\n"
        );
        assert_eq!(
            read_host_file(
                &root,
                Some(&json!(root.join("src").join("app.ts").to_string_lossy()))
            )
            .unwrap(),
            "source\n"
        );
        write_host_file(
            &root,
            Some(&json!("src/app.ts")),
            Some(&json!("source\n")),
            Some(&json!("edited\n")),
        )
        .unwrap();
        assert_eq!(
            read_host_file(&root, Some(&json!("src/app.ts"))).unwrap(),
            "edited\n"
        );
        let found = search_host_content(&root, &options(json!({ "query": "edited" }))).unwrap();
        assert_eq!(found["truncated"], false);
        assert_eq!(found["matches"].as_array().unwrap().len(), 1);
        assert_eq!(found["matches"][0]["relative"], "src/app.ts");
        assert_eq!(found["matches"][0]["line"], 1);
        assert_eq!(found["matches"][0]["column"], 1);
        assert_eq!(
            create_host_path(
                &root,
                Some(&json!("src")),
                Some(&json!("nested/new.ts")),
                Some(&json!(false))
            )
            .unwrap(),
            "src/nested/new.ts"
        );
        assert_eq!(
            read_host_file(&root, Some(&json!("src/nested/new.ts"))).unwrap(),
            ""
        );
        assert_eq!(
            create_host_path(
                &root,
                Some(&json!("")),
                Some(&json!("assets")),
                Some(&json!(true))
            )
            .unwrap(),
            "assets"
        );
        assert!(
            create_host_path(
                &root,
                Some(&json!("src")),
                Some(&json!("app.ts")),
                Some(&json!(false))
            )
            .is_err()
        );
        assert!(
            create_host_path(
                &root,
                Some(&json!("")),
                Some(&json!("../outside.txt")),
                Some(&json!(false))
            )
            .unwrap_err()
            .to_string()
            .contains("Invalid file name")
        );
        assert!(
            create_host_path(
                &root,
                Some(&json!("")),
                Some(&json!(".git/config")),
                Some(&json!(false))
            )
            .unwrap_err()
            .to_string()
            .contains("Invalid file name")
        );
        assert!(
            create_host_path(
                &root,
                Some(&json!("outside")),
                Some(&json!("bad.ts")),
                Some(&json!(false))
            )
            .unwrap_err()
            .to_string()
            .contains("outside")
        );
        assert!(
            write_host_file(
                &root,
                Some(&json!("src/app.ts")),
                Some(&json!("source\n")),
                Some(&json!("lost\n")),
            )
            .unwrap_err()
            .contains("changed on the host")
        );
        assert!(
            list_host_files(&root, Some(&json!("../")))
                .unwrap_err()
                .contains("outside")
        );
        assert!(
            list_host_files(&root, Some(&json!("outside")))
                .unwrap_err()
                .contains("outside")
        );
        assert!(
            read_host_file(&root, Some(&json!(".git/config")))
                .unwrap_err()
                .contains("outside")
        );
        let index = host_git_index(&root).unwrap();
        assert_eq!(index.branch, None);
        assert!(index.files.is_empty());
        assert_eq!((index.additions, index.deletions), (0, 0));
    }

    /// workspace.test.ts: "reports tracked and untracked changes and commits
    /// staged files".
    #[test]
    fn reports_tracked_and_untracked_changes_and_commits_staged_files() {
        let (_directory, root) = real_tempdir();
        let git = |args: &[&str]| run_git(&root, args);
        init(&root);
        std::fs::write(root.join("app.ts"), "before\n").unwrap();
        git(&["add", "--", "app.ts"]);
        git(&["commit", "-qm", "initial"]);
        std::fs::write(root.join("app.ts"), "after\n").unwrap();
        std::fs::write(root.join("new.ts"), "new\n").unwrap();

        let index = host_git_index(&root).unwrap();
        let search = |value: Value| search_host_content(&root, &options(value)).unwrap();
        let matches =
            search(json!({ "query": "AFTER", "include": "app.ts", "caseSensitive": false }));
        assert_eq!(matches["matches"][0]["relative"], "app.ts");
        assert_eq!(matches["matches"][0]["line"], 1);
        assert_eq!(matches["matches"][0]["column"], 1);
        assert_eq!(
            search(json!({ "query": "after", "exclude": "app.ts" }))["matches"],
            json!([])
        );
        assert_eq!(
            search(json!({ "query": "AFTER", "caseSensitive": true }))["matches"],
            json!([])
        );
        assert_eq!(
            search(json!({ "query": "aft", "wholeWord": true }))["matches"],
            json!([])
        );
        let regex = search(json!({ "query": "aft.r", "regex": true }));
        assert_eq!(regex["matches"].as_array().unwrap().len(), 1);
        assert_eq!(regex["matches"][0]["relative"], "app.ts");
        assert!(
            index.files.iter().any(|file| file.relative == "app.ts"
                && file.status == "modified"
                && file.unstaged)
        );
        assert!(
            index.files.iter().any(|file| file.relative == "new.ts"
                && file.status == "untracked"
                && file.unstaged)
        );
        let diff = host_file_diff(&root, Some(&json!("app.ts")), false).unwrap();
        assert_eq!(
            (diff.original.as_str(), diff.current.as_str()),
            ("before\n", "after\n")
        );
        assert!(
            host_git_action(
                &root,
                Some(&json!("stage")),
                Some(&json!("../outside")),
                None,
                None
            )
            .unwrap_err()
            .contains("outside")
        );
        host_git_action(&root, Some(&json!("stageAll")), None, None, None).unwrap();
        assert!(
            host_git_index(&root)
                .unwrap()
                .files
                .iter()
                .all(|file| file.staged)
        );
        let staged = host_file_diff(&root, Some(&json!("app.ts")), true).unwrap();
        assert_eq!(
            (staged.original.as_str(), staged.current.as_str()),
            ("before\n", "after\n")
        );
        host_git_action(
            &root,
            Some(&json!("commit")),
            None,
            Some(&json!("remote commit")),
            None,
        )
        .unwrap();
        assert!(host_git_index(&root).unwrap().files.is_empty());
        let (_remote_directory, remote) = real_tempdir();
        run_git(&remote, &["init", "--bare", "-q"]);
        git(&["remote", "add", "origin", &remote.to_string_lossy()]);
        git(&["push", "-q", "-u", "origin", "HEAD"]);
        std::fs::write(root.join("app.ts"), "another change\n").unwrap();
        git(&["add", "app.ts"]);
        git(&["commit", "-qm", "ahead"]);
        let index = host_git_index(&root).unwrap();
        assert_eq!(index.remote.as_deref(), Some("origin"));
        assert_eq!((index.ahead, index.behind), (1, 0));
        std::fs::write(root.join("app.ts"), "discard this\n").unwrap();
        host_git_action(
            &root,
            Some(&json!("discard")),
            Some(&json!("app.ts")),
            None,
            None,
        )
        .unwrap();
        assert!(host_git_index(&root).unwrap().files.is_empty());
    }

    /// workspace.test.ts: "stages and unstages a folder subtree without
    /// affecting other changes".
    #[test]
    fn stages_and_unstages_a_folder_subtree_without_affecting_other_changes() {
        let (_directory, root) = real_tempdir();
        let git = |args: &[&str]| run_git(&root, args);
        let action = |name: &str, path: &str| {
            host_git_action(&root, Some(&json!(name)), Some(&json!(path)), None, None).unwrap();
        };
        let file = |relative: &str| {
            host_git_index(&root)
                .unwrap()
                .files
                .into_iter()
                .find(|file| file.relative == relative)
                .unwrap()
        };
        init(&root);
        std::fs::create_dir_all(root.join("src/nested")).unwrap();
        std::fs::create_dir(root.join("src-other")).unwrap();
        std::fs::write(root.join("src/app.ts"), "before\n").unwrap();
        std::fs::write(root.join("src/nested/deleted.ts"), "delete me\n").unwrap();
        std::fs::write(root.join("src-other/app.ts"), "before\n").unwrap();
        std::fs::write(root.join("ready.txt"), "before\n").unwrap();
        std::fs::write(root.join(".gitignore"), "src/ignored.txt\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-qm", "initial"]);
        std::fs::write(root.join("src/app.ts"), "after\n").unwrap();
        std::fs::remove_dir_all(root.join("src/nested")).unwrap();
        std::fs::create_dir(root.join("src/added")).unwrap();
        std::fs::write(root.join("src/added/new.ts"), "new\n").unwrap();
        std::fs::write(root.join("src/ignored.txt"), "ignored\n").unwrap();
        std::fs::write(root.join("src-other/app.ts"), "outside\n").unwrap();
        std::fs::write(root.join("ready.txt"), "ready\n").unwrap();
        action("stage", "ready.txt");

        action("stage", "src");
        let files = host_git_index(&root).unwrap().files;
        assert_eq!(files.len(), 5);
        for file in &files {
            assert_eq!(
                file.staged,
                file.relative.starts_with("src/") || file.relative == "ready.txt"
            );
            assert_eq!(file.unstaged, file.relative == "src-other/app.ts");
        }

        action("unstage", "src");
        let files = host_git_index(&root).unwrap().files;
        assert_eq!(files.len(), 5);
        for file in &files {
            assert_eq!(file.staged, file.relative == "ready.txt");
            assert_eq!(file.unstaged, file.relative != "ready.txt");
        }
        assert_eq!(
            read_host_file(&root, Some(&json!("src/app.ts"))).unwrap(),
            "after\n"
        );

        // A folder can still appear in the Changes tree after it was deleted on disk.
        action("stage", "src/nested");
        let deleted = file("src/nested/deleted.ts");
        assert!(deleted.staged && !deleted.unstaged);
        action("unstage", "src/nested");
        let deleted = file("src/nested/deleted.ts");
        assert!(!deleted.staged && deleted.unstaged);
    }

    /// workspace.test.ts: "stages and unstages the literal folder %s without
    /// touching siblings".
    #[cfg(unix)]
    #[test]
    fn stages_and_unstages_literal_folders_without_touching_siblings() {
        for folder in ["*", "folder?", "[ab]", ":(glob)*"] {
            let (_directory, root) = real_tempdir();
            let git = |args: &[&str]| run_git(&root, args);
            let action = |name: &str, path: &str| {
                host_git_action(&root, Some(&json!(name)), Some(&json!(path)), None, None).unwrap();
            };
            let staged_paths = || {
                let output = std::process::Command::new("git")
                    .args(["diff", "--cached", "--name-only", "-z"])
                    .current_dir(&root)
                    .output()
                    .unwrap();
                let mut paths: Vec<String> = String::from_utf8_lossy(&output.stdout)
                    .split('\0')
                    .filter(|path| !path.is_empty())
                    .map(str::to_string)
                    .collect();
                paths.sort();
                paths
            };
            init(&root);
            let inside = format!("{folder}/inside.txt");
            for directory in [folder, "a", "folderx"] {
                std::fs::create_dir(root.join(directory)).unwrap();
            }
            let tracked = [
                inside.as_str(),
                "a/other.txt",
                "folderx/other.txt",
                "ready.txt",
            ];
            for path in tracked {
                std::fs::write(root.join(path), "before\n").unwrap();
            }
            git(&["add", "."]);
            git(&["commit", "-qm", "initial"]);
            for path in tracked {
                std::fs::write(root.join(path), "after\n").unwrap();
            }
            std::fs::write(root.join("private.txt"), "unrelated untracked data\n").unwrap();

            action("stage", "ready.txt");
            action("stage", folder);
            let mut expected = vec![inside.clone(), "ready.txt".to_string()];
            expected.sort();
            assert_eq!(staged_paths(), expected, "stage folder {folder}");

            action("unstage", folder);
            assert_eq!(staged_paths(), ["ready.txt"], "unstage folder {folder}");
            assert_eq!(
                read_host_file(&root, Some(&json!(inside))).unwrap(),
                "after\n"
            );
        }
    }

    /// workspace.test.ts: "stages selected host diff content without
    /// replacing the working file".
    #[test]
    fn stages_selected_host_diff_content_without_replacing_the_working_file() {
        let (_directory, root) = real_tempdir();
        let git = |args: &[&str]| run_git(&root, args);
        init(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/app.ts"), "one\ntwo\nthree\n").unwrap();
        git(&["add", "src/app.ts"]);
        git(&["commit", "-qm", "initial"]);
        std::fs::write(root.join("src/app.ts"), "ONE\ntwo\nTHREE\n").unwrap();
        host_git_action(
            &root,
            Some(&json!("stageContents")),
            Some(&json!("src/app.ts")),
            None,
            Some(&json!("ONE\ntwo\nthree\n")),
        )
        .unwrap();
        let shown = std::process::Command::new("git")
            .args(["show", ":src/app.ts"])
            .current_dir(&root)
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&shown.stdout), "ONE\ntwo\nthree\n");
        assert_eq!(
            read_host_file(&root, Some(&json!("src/app.ts"))).unwrap(),
            "ONE\ntwo\nTHREE\n"
        );
        host_git_action(
            &root,
            Some(&json!("discard")),
            Some(&json!("src/app.ts")),
            None,
            None,
        )
        .unwrap();
        assert_eq!(
            read_host_file(&root, Some(&json!("src/app.ts"))).unwrap(),
            "ONE\ntwo\nthree\n"
        );
        let listed = std::process::Command::new("git")
            .args(["ls-files"])
            .current_dir(&root)
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&listed.stdout).trim(), "src/app.ts");
        assert!(
            host_git_action(
                &root,
                Some(&json!("stageContents")),
                Some(&json!("../escape")),
                None,
                Some(&json!("x")),
            )
            .unwrap_err()
            .contains("outside")
        );
    }

    #[test]
    fn refuses_git_folders_and_the_root_where_a_file_is_needed() {
        let root = Path::new("/project");
        assert!(workspace_path(root, Some(&json!("")), false).is_err());
        assert!(workspace_path(root, Some(&json!("")), true).is_ok());
        assert!(workspace_path(root, Some(&json!("src/.GIT/x")), false).is_err());
        assert!(workspace_path(root, Some(&json!("/other/file")), false).is_err());
        assert_eq!(
            workspace_path(root, Some(&json!("src/../lib/a.ts")), false).unwrap(),
            PathBuf::from("/project/lib/a.ts")
        );
        assert!(workspace_path(root, Some(&json!(42)), false).is_err());
    }
}
