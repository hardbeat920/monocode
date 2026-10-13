//! Git-backed recordings of workspace changes during a turn. These snapshots
//! record what changed. Shared reviews require successful edit evidence for
//! each displayed file, without inferring which process owns individual lines.
use super::*;
use std::io::Write;

#[cfg(test)]
mod tests;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecordedTurn {
    id: String,
    before: Option<String>,
    after: Option<String>,
    head: Option<String>,
    shared: bool,
    error: Option<String>,
    #[serde(default)]
    dismissed: BTreeSet<String>,
    /// Files reported by this session's successful structured edit tools.
    /// Older shared snapshots have no attribution evidence and stay out of the card.
    #[serde(default)]
    edited: BTreeSet<String>,
}

#[derive(Serialize, Deserialize)]
struct TurnReview {
    cwd: String,
    repo: String,
    active: Option<RecordedTurn>,
    latest: Option<RecordedTurn>,
}

#[derive(Serialize, Deserialize)]
struct SavedTurn {
    cwd: String,
    repo: String,
    turn: RecordedTurn,
}

#[derive(Clone, PartialEq, Eq)]
struct Blob {
    mode: String,
    oid: String,
}

struct Change {
    relative: String,
    before: Option<Blob>,
    after: Option<Blob>,
    status: String,
    additions: i64,
    deletions: i64,
}

impl CheckpointStore {
    pub(super) fn begin_turn(&self, session_id: &str, cwd: &str, id: &str) -> Result<(), String> {
        let root = std::fs::canonicalize(project_root(cwd)?).map_err(|e| e.to_string())?;
        let discovered = run_git(&root, &["rev-parse", "--show-toplevel"]);
        if let Err(error) = &discovered {
            if error.contains("not a git repository")
                && read_review(&self.session_dir(session_id))?.is_none()
            {
                // Preserve structured-edit review for folders without Git.
                return self.ensure(session_id, cwd);
            }
        }
        let dir = self.session_dir(session_id);
        let mut review = read_review(&dir)?
            .filter(|saved| saved.cwd == path_to_js(&root))
            .unwrap_or(TurnReview {
                cwd: path_to_js(&root),
                repo: String::new(),
                active: None,
                latest: None,
            });
        let pending = review
            .active
            .clone()
            .filter(|turn| turn.before.is_some() && turn.error.is_none());
        if let Some(pending) = &pending {
            // A reload can interrupt checkpoint completion after the provider
            // already wrote files. Continue from that original boundary.
            self.finish_turn(session_id, cwd, &pending.id)?;
            review = read_review(&dir)?.ok_or("Missing continued turn review")?;
        }
        let mut turn = RecordedTurn {
            id: id.into(),
            before: pending.as_ref().and_then(|turn| turn.before.clone()),
            after: None,
            head: if let Some(pending) = &pending {
                pending.head.clone()
            } else {
                run_git(&root, &["rev-parse", "--verify", "HEAD"])
                    .ok()
                    .map(|bytes| String::from_utf8_lossy(&bytes).trim().to_string())
            },
            shared: pending.as_ref().is_some_and(|turn| turn.shared),
            error: None,
            dismissed: BTreeSet::new(),
            edited: pending
                .as_ref()
                .map(|turn| turn.edited.clone())
                .unwrap_or_default(),
        };
        // Publish the pending state first. A failed baseline must not leave the
        // previous turn's card underneath the new response.
        review.active = Some(turn.clone());
        write_review(&dir, &review)?;
        let capture = (|| {
            review.repo = String::from_utf8(discovered?)
                .map_err(|e| e.to_string())?
                .trim_end_matches(['\r', '\n'])
                .to_string();
            // Shared checkouts can include another session's writes. Keep the
            // recorded diff exact, but never offer to restore those turns.
            for (other_dir, mut other) in self.active_turns(&root, session_id)? {
                turn.shared = true;
                if let Some(active) = other.active.as_mut() {
                    active.shared = true;
                }
                write_review(&other_dir, &other)?;
            }
            let reference = turn_ref(session_id, id, "before");
            if let Some(before) = &turn.before {
                run_git(&root, &["update-ref", &reference, before])?;
                Ok(before.clone())
            } else {
                capture_workspace(&root, &reference)
            }
        })();
        match capture {
            Ok(oid) => turn.before = Some(oid),
            Err(error) => turn.error = Some(error),
        }
        review.active = Some(turn.clone());
        write_review(&dir, &review)?;
        match turn.error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    pub(super) fn finish_turn(&self, session_id: &str, cwd: &str, id: &str) -> Result<(), String> {
        let dir = self.session_dir(session_id);
        let Some(mut review) = self.matching_turn_review(session_id, cwd)? else {
            return Ok(());
        };
        let Some(mut turn) = review.active.take().filter(|turn| turn.id == id) else {
            return Ok(());
        };
        if turn.before.is_some() {
            match capture_workspace(Path::new(&review.cwd), &turn_ref(session_id, id, "after")) {
                Ok(oid) => turn.after = Some(oid),
                Err(error) => turn.error = Some(error),
            }
        }
        let error = turn.error.clone();
        write_json(
            &dir.join("turns").join(format!("{id}.json")),
            &SavedTurn {
                cwd: review.cwd.clone(),
                repo: review.repo.clone(),
                turn: turn.clone(),
            },
        )?;
        review.latest = Some(turn);
        write_review(&dir, &review)?;
        match error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    pub(super) fn has_turn_review(&self, session_id: &str, cwd: &str) -> Result<bool, String> {
        Ok(self.matching_turn_review(session_id, cwd)?.is_some())
    }

    pub(super) fn capture_turn_paths(
        &self,
        session_id: &str,
        cwd: &str,
        paths: &[String],
    ) -> Result<bool, String> {
        let Some(mut review) = self.matching_turn_review(session_id, cwd)? else {
            return Ok(false);
        };
        let Some(active) = review.active.as_mut() else {
            // Late tool events must not change the last completed turn.
            return Ok(true);
        };
        let workspace = project_root(cwd)?;
        let mut dirty = false;
        for path in paths {
            if let Ok(relative) = resolve_edit_path(&workspace, Path::new(&review.cwd), path) {
                dirty |= active.edited.insert(relative);
            }
        }
        if dirty {
            write_review(&self.session_dir(session_id), &review)?;
        }
        Ok(true)
    }

    /// Repair the interrupted-turn state left by versions that replaced its
    /// baseline on Continue. Only transcript-confirmed edits can be recovered.
    pub(super) fn recover_orphaned_turn(
        &self,
        session_id: &str,
        cwd: &str,
        paths: &[String],
    ) -> Result<(), String> {
        if paths.is_empty() {
            return Ok(());
        }
        let Some(mut review) = self.matching_turn_review(session_id, cwd)? else {
            return Ok(());
        };
        if review.active.is_some() {
            return Ok(());
        }
        let turn = completed_turn(&review)?;
        let root = Path::new(&review.cwd);
        if !turn.edited.is_empty() || !changes(root, turn)?.is_empty() {
            return Ok(());
        }
        let workspace = project_root(cwd)?;
        let edited: BTreeSet<_> = paths
            .iter()
            .filter_map(|path| resolve_edit_path(&workspace, root, path).ok())
            .collect();
        if edited.is_empty() {
            return Ok(());
        }
        let prefix = format!("refs/monocode/checkpoints/{session_id}/");
        let refs = run_git(
            root,
            &[
                "for-each-ref",
                "--sort=-committerdate",
                "--format=%(refname) %(objectname)",
                &prefix,
            ],
        )?;
        let refs: Vec<_> = String::from_utf8_lossy(&refs)
            .lines()
            .filter_map(|line| {
                line.split_once(' ')
                    .map(|(name, oid)| (name.to_string(), oid.to_string()))
            })
            .collect();
        let names: HashSet<_> = refs.iter().map(|(name, _)| name.as_str()).collect();
        let dir = self.session_dir(session_id);
        for (reference, before) in &refs {
            let Some(id) = reference
                .strip_prefix(&prefix)
                .and_then(|name| name.strip_suffix("/before"))
            else {
                continue;
            };
            if id == turn.id
                || names.contains(turn_ref(session_id, id, "after").as_str())
                || dir.join("turns").join(format!("{id}.json")).exists()
            {
                continue;
            }
            validate_id(id, "turn")?;
            let mut recovered = turn.clone();
            recovered.before = Some(before.clone());
            recovered.edited = edited.clone();
            // The orphan lost its original HEAD and overlap metadata. Review
            // remains available, but recovery must never make Undo available.
            recovered.head = None;
            recovered.shared = true;
            if !changes(root, &recovered)?
                .iter()
                .any(|change| edited.contains(&change.relative))
            {
                continue;
            }
            let mut archived = recovered.clone();
            archived.id = id.into();
            for saved in [archived, recovered.clone()] {
                write_json(
                    &dir.join("turns").join(format!("{}.json", saved.id)),
                    &SavedTurn {
                        cwd: review.cwd.clone(),
                        repo: review.repo.clone(),
                        turn: saved,
                    },
                )?;
            }
            review.latest = Some(recovered);
            return write_review(&dir, &review);
        }
        Ok(())
    }

    fn matching_turn_review(
        &self,
        session_id: &str,
        cwd: &str,
    ) -> Result<Option<TurnReview>, String> {
        let Some(review) = read_review(&self.session_dir(session_id))? else {
            return Ok(None);
        };
        let root = std::fs::canonicalize(project_root(cwd)?).map_err(|e| e.to_string())?;
        Ok((review.cwd == path_to_js(&root)).then_some(review))
    }

    fn active_turns(
        &self,
        root: &Path,
        except: &str,
    ) -> Result<Vec<(PathBuf, TurnReview)>, String> {
        let root = review_comparison_path(root);
        let mut active = Vec::new();
        if !self.root.exists() {
            return Ok(active);
        }
        for entry in std::fs::read_dir(&self.root).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            if entry.file_name() == except {
                continue;
            }
            if let Some(review) = read_review(&entry.path())? {
                let other = review_comparison_path(Path::new(&review.cwd));
                if review.active.is_some() && (root.starts_with(&other) || other.starts_with(&root))
                {
                    active.push((entry.path(), review));
                }
            }
        }
        Ok(active)
    }

    pub(super) fn turn_status(
        &self,
        session_id: &str,
        cwd: &str,
    ) -> Result<Option<CheckpointStatus>, String> {
        let Some(mut review) = self.matching_turn_review(session_id, cwd)? else {
            return Ok(None);
        };
        let turn = completed_turn(&review)?;
        let root = PathBuf::from(&review.cwd);
        let head = run_git(&root, &["rev-parse", "--verify", "HEAD"])
            .ok()
            .map(|bytes| String::from_utf8_lossy(&bytes).trim().to_string());
        let recorded = changes(&root, turn)?;
        let resolved = resolved_turn_paths(&root, turn, &recorded, head.as_deref())?;
        if !resolved.is_empty() {
            // Dismiss accepted work durably so later edits cannot resurrect
            // this turn's card. Its saved snapshots remain available to review.
            review.latest.as_mut().unwrap().dismissed.extend(resolved);
            write_review(&self.session_dir(session_id), &review)?;
        }
        let turn = completed_turn(&review)?;
        let can_restore =
            !turn.shared && head == turn.head && self.active_turns(&root, session_id)?.is_empty();
        let files = recorded
            .into_iter()
            .filter(|change| !turn.dismissed.contains(&change.relative))
            .filter(|change| !turn.shared || turn.edited.contains(&change.relative))
            .map(|change| {
                let undoable = can_restore && restorable(&root, &change);
                CheckpointFile {
                    path: path_to_js(&root.join(&change.relative)),
                    relative: change.relative,
                    status: change.status,
                    additions: change.additions,
                    deletions: change.deletions,
                    exact: true,
                    undoable,
                }
            })
            .collect();
        Ok(Some(CheckpointStatus { files }))
    }

    pub(super) fn turn_file_diff(
        &self,
        session_id: &str,
        cwd: &str,
        relative: &str,
    ) -> Result<Option<CheckpointFileDiff>, String> {
        let Some(review) = self.matching_turn_review(session_id, cwd)? else {
            return Ok(None);
        };
        let turn = completed_turn(&review)?;
        let root = Path::new(&review.cwd);
        let relative = resolve_turn_path(relative)?;
        let change = changes(root, turn)?
            .into_iter()
            .find(|change| change.relative == relative)
            .ok_or("This file did not change during the recorded turn")?;
        let original = read_blob(root, change.before.as_ref())?;
        let current = read_blob(root, change.after.as_ref())?;
        let too_large =
            matches!(original, FileState::Skipped) || matches!(current, FileState::Skipped);
        let binary = state_is_binary(&original) || state_is_binary(&current);
        let (original, current) = if too_large || binary {
            (String::new(), String::new())
        } else {
            (state_text(original), state_text(current))
        };
        Ok(Some(CheckpointFileDiff {
            path: path_to_js(&root.join(&relative)),
            relative,
            status: change.status,
            original,
            current,
            binary,
            too_large,
        }))
    }

    pub(super) fn keep_turn(
        &self,
        session_id: &str,
        cwd: &str,
        relative: Option<&str>,
    ) -> Result<Option<CheckpointStatus>, String> {
        let Some(mut review) = self.matching_turn_review(session_id, cwd)? else {
            return Ok(None);
        };
        let recorded = completed_turn(&review)?;
        let root = Path::new(&review.cwd);
        let dismissed = match relative {
            Some(relative) => vec![resolve_turn_path(relative)?],
            None => changes(root, recorded)?
                .into_iter()
                .map(|change| change.relative)
                .collect(),
        };
        review.latest.as_mut().unwrap().dismissed.extend(dismissed);
        write_review(&self.session_dir(session_id), &review)?;
        self.turn_status(session_id, cwd)
    }

    pub(super) fn undo_turn(
        &self,
        session_id: &str,
        cwd: &str,
        relative: Option<&str>,
    ) -> Result<Option<CheckpointStatus>, String> {
        let Some(review) = self.matching_turn_review(session_id, cwd)? else {
            return Ok(None);
        };
        let status = self.turn_status(session_id, cwd)?.unwrap();
        let root = Path::new(&review.cwd);
        let relative = relative.map(resolve_turn_path).transpose()?;
        let selected: Vec<_> = status
            .files
            .iter()
            .filter(|file| {
                relative
                    .as_ref()
                    .is_none_or(|relative| *relative == file.relative)
            })
            .collect();
        if selected.iter().any(|file| !file.undoable) {
            return Err("Cannot safely undo this turn: the workspace was shared, the branch moved, or a recorded file changed afterward".into());
        }
        let turn = completed_turn(&review)?;
        let selected: HashSet<_> = selected.iter().map(|file| file.relative.as_str()).collect();
        let changes: Vec<_> = changes(root, turn)?
            .into_iter()
            .filter(|change| selected.contains(change.relative.as_str()))
            .collect();
        // Validate all paths again before restoring any, preserving later edits
        // and refusing symlink traversal. The user's Git index is never reset.
        if changes.iter().any(|change| !restorable(root, change)) {
            return Err("Cannot safely undo: a recorded file changed after the checkpoint".into());
        }
        for change in changes {
            if change.before.is_some() {
                run_git(
                    root,
                    &[
                        "restore",
                        "--source",
                        turn.before.as_deref().unwrap(),
                        "--worktree",
                        "--",
                        &change.relative,
                    ],
                )?;
            } else {
                std::fs::remove_file(root.join(&change.relative)).map_err(|e| e.to_string())?;
            }
        }
        self.keep_turn(session_id, cwd, relative.as_deref())
    }

    pub(super) fn forget_turns(&self, session_id: &str) -> Result<(), String> {
        let dir = self.session_dir(session_id);
        let mut repos = BTreeSet::new();
        if let Some(review) = read_review(&dir)? {
            repos.insert(review.repo);
        }
        let history = dir.join("turns");
        if history.exists() {
            for entry in std::fs::read_dir(history).map_err(|e| e.to_string())? {
                let saved: SavedTurn = serde_json::from_slice(
                    &std::fs::read(entry.map_err(|e| e.to_string())?.path())
                        .map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
                repos.insert(saved.repo);
            }
        }
        let prefix = format!("refs/monocode/checkpoints/{session_id}/");
        for repo in repos {
            if !Path::new(&repo).is_dir() {
                continue;
            }
            let refs = run_git(
                Path::new(&repo),
                &["for-each-ref", "--format=%(refname)", &prefix],
            )?;
            for reference in String::from_utf8_lossy(&refs)
                .lines()
                .filter(|reference| reference.starts_with(&prefix))
            {
                run_git(Path::new(&repo), &["update-ref", "-d", reference])?;
            }
        }
        Ok(())
    }
}

fn review_comparison_path(path: &Path) -> PathBuf {
    // path_to_js replaces Windows separators, including in canonical verbatim
    // paths. Restore native separators before comparing path components.
    if cfg!(windows) {
        PathBuf::from(path.to_string_lossy().replace('/', "\\"))
    } else {
        path.to_path_buf()
    }
}

fn completed_turn(review: &TurnReview) -> Result<&RecordedTurn, String> {
    let turn = review
        .active
        .as_ref()
        .or(review.latest.as_ref())
        .ok_or("No recorded turn is available")?;
    if let Some(error) = &turn.error {
        return Err(format!("Could not record workspace changes: {error}"));
    }
    if review.active.is_some() {
        return Err("The turn ended without a final workspace checkpoint".into());
    }
    if turn.before.is_none() || turn.after.is_none() {
        return Err("The turn's workspace checkpoints are incomplete".into());
    }
    Ok(turn)
}

fn turn_ref(session: &str, turn: &str, side: &str) -> String {
    format!("refs/monocode/checkpoints/{session}/{turn}/{side}")
}

// Git's NUL-delimited output already contains literal paths. Never trim them
// or interpret rename notation: those characters can be part of a filename.
fn resolve_turn_path(relative: &str) -> Result<String, String> {
    if relative.is_empty()
        || relative.contains('\0')
        || Path::new(relative).is_absolute()
        || Path::new(relative)
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
        || relative
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".." | ".git"))
    {
        return Err("Invalid checkpoint path".into());
    }
    Ok(relative.into())
}

fn resolve_edit_path(workspace: &Path, root: &Path, path: &str) -> Result<String, String> {
    let path = expand_home(path);
    let relative = if path.is_absolute() {
        let relative = path
            .strip_prefix(workspace)
            .or_else(|_| path.strip_prefix(root))
            .map_err(|_| "Path is outside the project")?;
        path_to_js(relative)
    } else {
        path_to_js(&path)
    };
    resolve_turn_path(&relative)
}

fn git_command(root: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("git");
    crate::hide_window_console(&mut command);
    command
        .args(["--no-pager", "-C"])
        .arg(root)
        .args(["-c", "core.fsmonitor=false"])
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_LITERAL_PATHSPECS", "1")
        .env("PATH", crate::harness::gui_search_path());
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
    ] {
        command.env_remove(name);
    }
    command
}

fn checked_output(command: &mut Command) -> Result<Vec<u8>, String> {
    let output = command.output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(output.stdout)
}

fn run_git(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    checked_output(&mut git_command(root, args))
}

struct PrivateIndex(PathBuf);
impl Drop for PrivateIndex {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        let _ = std::fs::remove_file(self.0.with_extension("lock"));
    }
}

fn capture_workspace(root: &Path, reference: &str) -> Result<String, String> {
    if run_git(root, &["config", "--bool", "core.sparseCheckout"])
        .ok()
        .is_some_and(|bytes| bytes.starts_with(b"true"))
    {
        return Err("Workspace checkpoints are unavailable for sparse checkouts".into());
    }
    let index = PrivateIndex(
        std::env::temp_dir().join(format!("monocode-checkpoint-{}", uuid::Uuid::new_v4())),
    );
    let run = |args: &[&str]| {
        checked_output(
            git_command(root, args)
                .env("GIT_INDEX_FILE", &index.0)
                .env("GIT_AUTHOR_NAME", "MonoCode")
                .env("GIT_AUTHOR_EMAIL", "checkpoint@monocode.local")
                .env("GIT_COMMITTER_NAME", "MonoCode")
                .env("GIT_COMMITTER_EMAIL", "checkpoint@monocode.local"),
        )
    };
    if run_git(root, &["rev-parse", "--verify", "HEAD"]).is_ok() {
        run(&["read-tree", "HEAD"])?;
    } else {
        run(&["read-tree", "--empty"])?;
    }
    let durable = [
        "-c",
        "core.fsync=objects,reference",
        "-c",
        "core.fsyncMethod=fsync",
    ];
    run(&[durable.as_slice(), &["add", "-A", "--", "."]].concat())?;
    let tree = String::from_utf8(run(&[durable.as_slice(), &["write-tree"]].concat())?)
        .map_err(|e| e.to_string())?;
    let commit = String::from_utf8(run(&[
        durable.as_slice(),
        &[
            "commit-tree",
            tree.trim(),
            "-m",
            "MonoCode workspace checkpoint",
        ],
    ]
    .concat())?)
    .map_err(|e| e.to_string())?;
    run(&[
        durable.as_slice(),
        &["update-ref", reference, commit.trim()],
    ]
    .concat())?;
    Ok(commit.trim().to_string())
}

fn changes(root: &Path, turn: &RecordedTurn) -> Result<Vec<Change>, String> {
    let before = turn.before.as_deref().ok_or("Missing turn baseline")?;
    let after = turn.after.as_deref().ok_or("Missing turn result")?;
    let args = [
        "diff",
        "--relative",
        "--no-ext-diff",
        "--no-textconv",
        "--no-renames",
        "--no-abbrev",
        "-z",
    ];
    let raw = run_git(
        root,
        &[args.as_slice(), &["--raw", before, after, "--", "."]].concat(),
    )?;
    let numstat = run_git(
        root,
        &[args.as_slice(), &["--numstat", before, after, "--", "."]].concat(),
    )?;
    let mut stats = BTreeMap::new();
    for row in numstat
        .split(|byte| *byte == 0)
        .filter(|row| !row.is_empty())
    {
        let mut fields = row.splitn(3, |byte| *byte == b'\t');
        let additions = fields.next().ok_or("Invalid checkpoint line counts")?;
        let deletions = fields.next().ok_or("Invalid checkpoint line counts")?;
        let path = fields.next().ok_or("Invalid checkpoint path")?;
        let count = |bytes: &[u8]| -> Result<i64, String> {
            if bytes == b"-" {
                return Ok(0);
            }
            String::from_utf8_lossy(bytes)
                .parse()
                .map_err(|_| "Invalid checkpoint line counts".into())
        };
        stats.insert(path.to_vec(), (count(additions)?, count(deletions)?));
    }
    let mut records = raw.split(|byte| *byte == 0).filter(|row| !row.is_empty());
    let mut out = Vec::new();
    while let Some(header) = records.next() {
        let path = records.next().ok_or("Invalid checkpoint path")?;
        let fields: Vec<_> = std::str::from_utf8(header)
            .map_err(|e| e.to_string())?
            .split_whitespace()
            .collect();
        if fields.len() != 5 || !fields[0].starts_with(':') {
            return Err("Invalid checkpoint file status".into());
        }
        let blob = |mode: &str, oid: &str| {
            (mode != "000000").then(|| Blob {
                mode: mode.into(),
                oid: oid.into(),
            })
        };
        let (additions, deletions) = stats.remove(path).ok_or("Missing checkpoint line counts")?;
        out.push(Change {
            relative: String::from_utf8(path.to_vec()).map_err(|e| e.to_string())?,
            before: blob(&fields[0][1..], fields[2]),
            after: blob(fields[1], fields[3]),
            status: match fields[4] {
                "A" => "added",
                "D" => "deleted",
                _ => "modified",
            }
            .into(),
            additions,
            deletions,
        });
    }
    out.sort_by(|a, b| a.relative.cmp(&b.relative));
    Ok(out)
}

fn resolved_turn_paths(
    root: &Path,
    turn: &RecordedTurn,
    recorded: &[Change],
    head: Option<&str>,
) -> Result<Vec<String>, String> {
    let Some(head) = head.filter(|head| Some(*head) != turn.head.as_deref()) else {
        return Ok(Vec::new());
    };
    if recorded
        .iter()
        .all(|change| turn.dismissed.contains(&change.relative))
    {
        return Ok(Vec::new());
    }
    let diff = [
        "diff",
        "--relative",
        "--no-ext-diff",
        "--no-textconv",
        "--no-renames",
        "--name-only",
        "-z",
    ];
    let after = turn.after.as_deref().ok_or("Missing turn result")?;
    let differs_from_result =
        git_paths(root, &[diff.as_slice(), &[after, head, "--", "."]].concat())?;
    let mut pending = git_paths(root, &[diff.as_slice(), &[head, "--", "."]].concat())?;
    pending.extend(git_paths(
        root,
        &[
            "ls-files",
            "--others",
            "--exclude-standard",
            "-z",
            "--",
            ".",
        ],
    )?);
    // A file is resolved when its recorded result is in HEAD, even if someone
    // edited it again, or when it is clean after a commit that included more edits.
    Ok(recorded
        .iter()
        .filter(|change| {
            !turn.dismissed.contains(&change.relative)
                && (!differs_from_result.contains(&change.relative)
                    || !pending.contains(&change.relative))
        })
        .map(|change| change.relative.clone())
        .collect())
}

fn git_paths(root: &Path, args: &[&str]) -> Result<HashSet<String>, String> {
    run_git(root, args)?
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| String::from_utf8(path.to_vec()).map_err(|e| e.to_string()))
        .collect()
}

fn restorable(root: &Path, change: &Change) -> bool {
    if path_contains_symlink(root, &change.relative)
        || [change.before.as_ref(), change.after.as_ref()]
            .into_iter()
            .flatten()
            .any(|blob| !matches!(blob.mode.as_str(), "100644" | "100755"))
    {
        return false;
    }
    let path = root.join(&change.relative);
    match (&change.after, std::fs::symlink_metadata(&path)) {
        (None, Err(error)) => error.kind() == std::io::ErrorKind::NotFound,
        (Some(after), Ok(meta)) if meta.is_file() => {
            let mode = if file_mode(&path).is_some_and(|mode| mode & 0o111 != 0) {
                "100755"
            } else {
                "100644"
            };
            mode == after.mode
                && run_git(
                    root,
                    &[
                        "hash-object",
                        &format!("--path={}", change.relative),
                        "--",
                        &change.relative,
                    ],
                )
                .is_ok_and(|bytes| String::from_utf8_lossy(&bytes).trim() == after.oid)
        }
        _ => false,
    }
}

fn read_blob(root: &Path, blob: Option<&Blob>) -> Result<FileState, String> {
    let Some(blob) = blob else {
        return Ok(FileState::Missing);
    };
    if blob.mode == "160000" {
        return Ok(FileState::Contents(
            format!("Subproject commit {}\n", blob.oid).into_bytes(),
        ));
    }
    let size = run_git(root, &["cat-file", "-s", &blob.oid])?;
    let size: u64 = String::from_utf8_lossy(&size)
        .trim()
        .parse()
        .map_err(|_| "Invalid checkpoint blob size")?;
    if size > MAX_TEXT_FILE_BYTES {
        return Ok(FileState::Skipped);
    }
    Ok(FileState::Contents(run_git(
        root,
        &["cat-file", "blob", &blob.oid],
    )?))
}

fn read_review(dir: &Path) -> Result<Option<TurnReview>, String> {
    match std::fs::read(dir.join("turn-review.json")) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| e.to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

fn write_review(dir: &Path, review: &TurnReview) -> Result<(), String> {
    write_json(&dir.join("turn-review.json"), review)
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let parent = path.parent().ok_or("Invalid checkpoint metadata path")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    let mut file = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
    file.write_all(&serde_json::to_vec(value).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    std::fs::rename(tmp, path).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    std::fs::File::open(parent)
        .and_then(|dir| dir.sync_all())
        .map_err(|e| e.to_string())?;
    Ok(())
}
