use super::*;

pub(super) fn action(
    root: &Path,
    action: &str,
    mut args: serde_json::Value,
) -> Result<Option<serde_json::Value>, String> {
    args["searchPath"] = json!(shell::cached_gui_search_path());
    monocode_isolation::publication(root, action, args)
}

pub(super) struct Trees {
    pub head: String,
    pub index: String,
    pub worktree: String,
}
pub(super) fn trees(root: &Path) -> Result<Option<Trees>, String> {
    action(root, "trees", json!({}))?
        .map(|value| {
            let string = |key: &str| {
                value[key]
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| "Invalid publication snapshot".to_string())
            };
            Ok(Trees {
                head: string("head")?,
                index: string("index")?,
                worktree: string("worktree")?,
            })
        })
        .transpose()
}
pub(super) fn prepare(root: &Path) -> Result<bool, String> {
    if action(root, "canonicalize", json!({}))?.is_none() {
        return Ok(false);
    }
    action(root, "prepare", json!({}))?;
    Ok(true)
}
pub(super) fn canonicalize(root: &Path) -> Result<bool, String> {
    Ok(action(root, "canonicalize", json!({}))?.is_some())
}
pub(super) fn stage(
    root: &Path,
    mode: &str,
    relative: Option<&str>,
    contents: Option<&[u8]>,
) -> Result<bool, String> {
    if contents.is_some() && action(root, "initialize", json!({}))?.is_none() {
        return Ok(false);
    }
    let contents = contents
        .map(std::str::from_utf8)
        .transpose()
        .map_err(|_| "File is not valid UTF-8")?;
    Ok(action(
        root,
        "stage",
        json!({"mode":mode,"relative":relative,"contents":contents}),
    )?
    .is_some())
}
pub(super) fn discard(root: &Path, relative: Option<&str>) -> Result<bool, String> {
    Ok(action(root, "discard", json!({"relative":relative}))?.is_some())
}

pub(super) fn index(root: &Path, include_sync: bool) -> Result<Option<DiffIndex>, String> {
    let Some(trees) = trees(root)? else {
        return Ok(None);
    };
    let mut files = HashMap::<String, FileAcc>::new();
    let mut statuses = HashMap::new();
    if let Some(counts) = git_run(
        root,
        &[
            "diff",
            "--no-ext-diff",
            "--no-renames",
            "--relative",
            "--numstat",
            "-z",
            &trees.head,
            &trees.worktree,
            "--",
            ".",
        ],
    ) {
        add_numstat_map(&counts, &mut files);
    }
    for (from, to, staged) in [
        (&trees.head, &trees.index, true),
        (&trees.index, &trees.worktree, false),
    ] {
        let names = git_run(
            root,
            &[
                "diff",
                "--no-ext-diff",
                "--no-renames",
                "--relative",
                "--name-status",
                "-z",
                from,
                to,
                "--",
                ".",
            ],
        )
        .ok_or("Cannot read copy publication changes")?;
        let mut changed = HashMap::new();
        add_name_status(&names, &mut changed);
        for (path, status) in changed {
            let file = files.entry(path.clone()).or_default();
            if staged {
                file.staged = true;
            } else {
                file.unstaged = true;
            }
            statuses.insert(path, status);
        }
    }
    let mut files: Vec<_> = files
        .into_iter()
        .map(|(relative, file)| ChangedFile {
            path: path_to_js(&root.join(&relative)),
            status: statuses
                .get(&relative)
                .copied()
                .unwrap_or("modified")
                .into(),
            relative,
            additions: file.additions,
            deletions: file.deletions,
            staged: file.staged,
            unstaged: file.unstaged,
        })
        .collect();
    files.sort_by(|a, b| a.relative.cmp(&b.relative));
    let sync = if include_sync {
        sync_for(root)
    } else {
        Sync::default()
    };
    Ok(Some(DiffIndex {
        branch: git_branch(root),
        head: git_stdout(root, &["rev-parse", "HEAD"]),
        additions: files.iter().map(|f| f.additions).sum(),
        deletions: files.iter().map(|f| f.deletions).sum(),
        files,
        remote: sync.remote,
        upstream: sync.upstream,
        default_branch: sync.default_branch,
        ahead: sync.ahead,
        behind: sync.behind,
        ahead_of_default: sync.ahead_of_default,
        head_pushed: sync.head_pushed,
    }))
}
