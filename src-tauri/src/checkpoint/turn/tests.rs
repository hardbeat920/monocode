use super::*;

struct Fixture {
    dir: PathBuf,
    repo: PathBuf,
    store: CheckpointStore,
}

impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("monocode-turn-test-{}", uuid::Uuid::new_v4()));
        let repo = dir.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        run_git(&repo, &["init"]).unwrap();
        run_git(&repo, &["config", "user.name", "Test"]).unwrap();
        run_git(&repo, &["config", "user.email", "test@example.invalid"]).unwrap();
        run_git(&repo, &["config", "core.autocrlf", "false"]).unwrap();
        std::fs::write(repo.join("a.txt"), "original\n").unwrap();
        std::fs::write(repo.join("delete.txt"), "remove me\n").unwrap();
        run_git(&repo, &["add", "."]).unwrap();
        run_git(&repo, &["commit", "-m", "initial"]).unwrap();
        Self {
            store: CheckpointStore::new(dir.join("checkpoints")),
            dir,
            repo,
        }
    }
    fn cwd(&self) -> &str {
        self.repo.to_str().unwrap()
    }
    fn write(&self, path: &str, text: &str) {
        let path = self.repo.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn records_shell_changes_and_preserves_the_index_head_and_saved_result() {
    let f = Fixture::new();
    f.write("a.txt", "user change\n");
    f.write("user.txt", "existing untracked work\n");
    run_git(&f.repo, &["add", "a.txt"]).unwrap();
    let index = std::fs::read(f.repo.join(".git/index")).unwrap();
    let head = run_git(&f.repo, &["rev-parse", "HEAD"]).unwrap();
    f.store.begin_turn("session", f.cwd(), "first").unwrap();
    // No structured edit events: the actual file state is authoritative.
    f.write("a.txt", "user change\nagent change\n");
    f.write("new.txt", "one\ntwo\n");
    std::fs::remove_file(f.repo.join("delete.txt")).unwrap();
    f.store.finish_turn("session", f.cwd(), "first").unwrap();
    let status = f.store.status("session", f.cwd()).unwrap();
    assert_eq!(
        status
            .files
            .iter()
            .map(|file| file.relative.as_str())
            .collect::<Vec<_>>(),
        ["a.txt", "delete.txt", "new.txt"]
    );
    assert_eq!(
        (status.files[0].additions, status.files[0].deletions),
        (1, 0)
    );
    assert!(status.files.iter().all(|file| file.exact && file.undoable));
    assert_eq!(std::fs::read(f.repo.join(".git/index")).unwrap(), index);
    assert_eq!(run_git(&f.repo, &["rev-parse", "HEAD"]).unwrap(), head);

    f.write("a.txt", "a later editor change\n");
    // Reopening the store proves the review survives a restart and never reads
    // later contents as though they were this turn's result.
    let reopened = CheckpointStore::new(f.dir.join("checkpoints"));
    let diff = reopened.file_diff("session", f.cwd(), "a.txt").unwrap();
    assert_eq!(diff.original, "user change\n");
    assert_eq!(diff.current, "user change\nagent change\n");
    let status = reopened.status("session", f.cwd()).unwrap();
    assert!(status.files[0].exact);
    assert!(!status.files[0].undoable);
    assert!(reopened.undo("session", f.cwd(), None).is_err());
    assert!(f.repo.join("new.txt").exists());
    assert!(!f.repo.join("delete.txt").exists());
}

#[test]
fn each_turn_has_a_fresh_baseline_and_noop_turns_clear_the_old_card() {
    let f = Fixture::new();
    f.store.begin_turn("session", f.cwd(), "first").unwrap();
    f.write("a.txt", "first turn\n");
    f.store.finish_turn("session", f.cwd(), "first").unwrap();
    f.write("a.txt", "external edit between turns\n");
    f.store.begin_turn("session", f.cwd(), "second").unwrap();
    f.write("a.txt", "external edit between turns\nsecond turn\n");
    f.store.finish_turn("session", f.cwd(), "second").unwrap();
    let diff = f.store.file_diff("session", f.cwd(), "a.txt").unwrap();
    assert_eq!(diff.original, "external edit between turns\n");
    assert_eq!(diff.current, "external edit between turns\nsecond turn\n");
    let status = f.store.status("session", f.cwd()).unwrap();
    assert_eq!(
        (status.files[0].additions, status.files[0].deletions),
        (1, 0)
    );
    f.store.begin_turn("session", f.cwd(), "third").unwrap();
    f.store.finish_turn("session", f.cwd(), "third").unwrap();
    assert!(f.store.status("session", f.cwd()).unwrap().files.is_empty());
    assert!(f.dir.join("checkpoints/session/turns/first.json").exists());
    assert!(run_git(
        &f.repo,
        &["rev-parse", &turn_ref("session", "first", "after")]
    )
    .is_ok());
}

#[test]
fn overlapping_turns_keep_exact_diffs_but_cannot_restore_shared_work() {
    let f = Fixture::new();
    f.write("a.txt", "top\n\nbottom\n");
    f.store.begin_turn("a", f.cwd(), "first").unwrap();
    f.store.begin_turn("b", f.cwd(), "second").unwrap();
    f.write("a.txt", "top edited by a\n\nbottom edited by b\n");
    f.store.finish_turn("a", f.cwd(), "first").unwrap();
    f.store.finish_turn("b", f.cwd(), "second").unwrap();
    for id in ["a", "b"] {
        let status = f.store.status(id, f.cwd()).unwrap();
        assert_eq!(
            (status.files[0].additions, status.files[0].deletions),
            (2, 2)
        );
        assert!(status.files[0].exact);
        assert!(!status.files[0].undoable);
        assert!(f.store.undo(id, f.cwd(), None).is_err());
        assert_eq!(
            f.store.file_diff(id, f.cwd(), "a.txt").unwrap().current,
            "top edited by a\n\nbottom edited by b\n"
        );
    }
}

#[test]
fn undo_restores_only_recorded_files_and_keep_retains_history() {
    let f = Fixture::new();
    f.write("user.txt", "user baseline\n");
    f.store.begin_turn("session", f.cwd(), "first").unwrap();
    f.write("a.txt", "agent\n");
    f.write("user.txt", "user baseline\nagent addition\n");
    f.write("new.txt", "created\n");
    std::fs::remove_file(f.repo.join("delete.txt")).unwrap();
    f.store.finish_turn("session", f.cwd(), "first").unwrap();
    f.write("unrelated.txt", "later work\n");
    let index = std::fs::read(f.repo.join(".git/index")).unwrap();
    f.store.keep("session", f.cwd(), Some("a.txt")).unwrap();
    f.store.undo("session", f.cwd(), None).unwrap();
    assert_eq!(
        std::fs::read_to_string(f.repo.join("a.txt")).unwrap(),
        "agent\n"
    );
    assert_eq!(
        std::fs::read_to_string(f.repo.join("user.txt")).unwrap(),
        "user baseline\n"
    );
    assert_eq!(
        std::fs::read_to_string(f.repo.join("delete.txt")).unwrap(),
        "remove me\n"
    );
    assert!(f.repo.join("unrelated.txt").exists());
    assert!(!f.repo.join("new.txt").exists());
    assert!(f.store.status("session", f.cwd()).unwrap().files.is_empty());
    assert_eq!(std::fs::read(f.repo.join(".git/index")).unwrap(), index);
    assert!(f.dir.join("checkpoints/session/turns/first.json").exists());
    f.store.forget("session").unwrap();
    assert!(run_git(
        &f.repo,
        &["for-each-ref", "refs/monocode/checkpoints/session/"]
    )
    .unwrap()
    .is_empty());
}

#[test]
fn failures_and_incomplete_turns_never_fall_back_to_the_previous_result() {
    let f = Fixture::new();
    f.store.begin_turn("session", f.cwd(), "first").unwrap();
    f.write("a.txt", "first\n");
    f.store.finish_turn("session", f.cwd(), "first").unwrap();
    f.store.begin_turn("session", f.cwd(), "second").unwrap();
    assert!(f
        .store
        .status("session", f.cwd())
        .unwrap_err()
        .contains("without a final"));
    // Stale completion from a cancelled turn must not settle a newer turn.
    f.store.finish_turn("session", f.cwd(), "first").unwrap();
    assert!(f.store.status("session", f.cwd()).is_err());
    run_git(&f.repo, &["config", "core.sparseCheckout", "true"]).unwrap();
    assert!(f.store.finish_turn("session", f.cwd(), "second").is_err());
    assert!(f
        .store
        .status("session", f.cwd())
        .unwrap_err()
        .contains("sparse"));
    run_git(&f.repo, &["config", "core.sparseCheckout", "false"]).unwrap();
    f.store.begin_turn("session", f.cwd(), "third").unwrap();
    f.store.finish_turn("session", f.cwd(), "third").unwrap();
    assert!(f.store.status("session", f.cwd()).unwrap().files.is_empty());
}

#[test]
fn records_empty_binary_and_unusual_paths_without_the_old_file_limit() {
    let f = Fixture::new();
    f.write(".gitignore", "ignored/\n");
    f.store.begin_turn("session", f.cwd(), "first").unwrap();
    f.write("ignored/file.txt", "ignored\n");
    for index in 0..501 {
        f.write(&format!("new/{index}.txt"), "");
    }
    f.write("binary.bin", "\0binary");
    let path = if cfg!(windows) {
        "café name.txt"
    } else {
        "café\tname\n.txt"
    };
    f.write(path, "new\n");
    f.store.finish_turn("session", f.cwd(), "first").unwrap();
    let status = f.store.status("session", f.cwd()).unwrap();
    assert_eq!(status.files.len(), 503);
    assert!(status
        .files
        .iter()
        .any(|file| file.relative == path && file.additions == 1));
    assert!(
        f.store
            .file_diff("session", f.cwd(), "binary.bin")
            .unwrap()
            .binary
    );
    assert_eq!(
        f.store.file_diff("session", f.cwd(), path).unwrap().current,
        "new\n"
    );
}

#[test]
fn records_committed_changes_without_reading_the_current_git_status() {
    let f = Fixture::new();
    f.store.begin_turn("session", f.cwd(), "first").unwrap();
    f.write("a.txt", "committed during the turn\n");
    run_git(&f.repo, &["add", "."]).unwrap();
    run_git(&f.repo, &["commit", "-m", "agent changes"]).unwrap();
    f.store.finish_turn("session", f.cwd(), "first").unwrap();
    assert!(run_git(&f.repo, &["status", "--porcelain"])
        .unwrap()
        .is_empty());
    let status = f.store.status("session", f.cwd()).unwrap();
    assert_eq!(status.files.len(), 1);
    assert_eq!(
        (status.files[0].additions, status.files[0].deletions),
        (1, 1)
    );
    assert!(status.files[0].exact);
    assert!(!status.files[0].undoable);
    assert!(f.store.undo("session", f.cwd(), None).is_err());
}

#[test]
fn linked_worktrees_have_independent_recordings_and_indexes() {
    let f = Fixture::new();
    let linked = f.dir.join("linked");
    run_git(
        &f.repo,
        &["worktree", "add", "-b", "linked", linked.to_str().unwrap()],
    )
    .unwrap();
    let cwd = linked.to_str().unwrap();
    let git_dir = run_git(&linked, &["rev-parse", "--absolute-git-dir"]).unwrap();
    let index_path = PathBuf::from(String::from_utf8(git_dir).unwrap().trim()).join("index");
    let index = std::fs::read(&index_path).unwrap();
    f.store.begin_turn("main", f.cwd(), "first").unwrap();
    f.store.begin_turn("linked", cwd, "second").unwrap();
    f.write("a.txt", "main change\n");
    std::fs::write(linked.join("delete.txt"), "linked change\n").unwrap();
    f.store.finish_turn("main", f.cwd(), "first").unwrap();
    f.store.finish_turn("linked", cwd, "second").unwrap();
    assert_eq!(
        f.store.status("main", f.cwd()).unwrap().files[0].relative,
        "a.txt"
    );
    let status = f.store.status("linked", cwd).unwrap();
    assert_eq!(status.files.len(), 1);
    assert_eq!(status.files[0].relative, "delete.txt");
    assert!(status.files[0].undoable);
    assert_eq!(std::fs::read(&index_path).unwrap(), index);
    f.store.undo("linked", cwd, None).unwrap();
    assert_eq!(
        std::fs::read_to_string(f.repo.join("a.txt")).unwrap(),
        "main change\n"
    );
}

#[cfg(unix)]
#[test]
fn preserves_literal_paths_and_records_modes_and_symlinks_safely() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let f = Fixture::new();
    run_git(&f.repo, &["config", "core.fileMode", "true"]).unwrap();
    let path = " literal => name .txt ";
    f.write(path, "baseline\n");
    f.store.begin_turn("session", f.cwd(), "first").unwrap();
    f.write(path, "changed\n");
    std::fs::set_permissions(f.repo.join("a.txt"), std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::rename(f.repo.join("delete.txt"), f.repo.join("renamed.txt")).unwrap();
    symlink("a.txt", f.repo.join("link.txt")).unwrap();
    f.store.finish_turn("session", f.cwd(), "first").unwrap();
    let status = f.store.status("session", f.cwd()).unwrap();
    assert_eq!(status.files.len(), 5);
    let link = status
        .files
        .iter()
        .find(|file| file.relative == "link.txt")
        .unwrap();
    assert!(link.exact);
    assert!(!link.undoable);
    let diff = f.store.file_diff("session", f.cwd(), path).unwrap();
    assert_eq!(diff.original, "baseline\n");
    assert_eq!(diff.current, "changed\n");
    f.store.undo("session", f.cwd(), Some(path)).unwrap();
    assert_eq!(
        std::fs::read_to_string(f.repo.join(path)).unwrap(),
        "baseline\n"
    );
    f.store.undo("session", f.cwd(), Some("a.txt")).unwrap();
    assert_eq!(file_mode(&f.repo.join("a.txt")).unwrap() & 0o111, 0);
    for path in ["../a.txt", "/a.txt", "a/../a.txt", "./a.txt", ".git/config"] {
        assert!(f.store.file_diff("session", f.cwd(), path).is_err());
    }
}

#[test]
fn checkpoints_scope_nested_workspaces_and_repositories_without_head() {
    let f = Fixture::new();
    f.write("nested/a.txt", "nested\n");
    let cwd = f.repo.join("nested");
    let cwd = cwd.to_str().unwrap();
    f.store.begin_turn("nested", cwd, "first").unwrap();
    f.write("nested/a.txt", "nested\nchanged\n");
    f.write("a.txt", "outside scope\n");
    f.store.finish_turn("nested", cwd, "first").unwrap();
    let status = f.store.status("nested", cwd).unwrap();
    assert_eq!(status.files.len(), 1);
    assert_eq!(status.files[0].relative, "a.txt");
    assert_eq!(
        f.store.file_diff("nested", cwd, "a.txt").unwrap().current,
        "nested\nchanged\n"
    );
    f.store.undo("nested", cwd, None).unwrap();
    assert_eq!(
        std::fs::read_to_string(f.repo.join("a.txt")).unwrap(),
        "outside scope\n"
    );

    let unborn = f.dir.join("unborn");
    std::fs::create_dir(&unborn).unwrap();
    run_git(&unborn, &["init"]).unwrap();
    let cwd = unborn.to_str().unwrap();
    f.store.begin_turn("unborn", cwd, "first").unwrap();
    std::fs::write(unborn.join("new.txt"), "new\n").unwrap();
    f.store.finish_turn("unborn", cwd, "first").unwrap();
    assert_eq!(f.store.status("unborn", cwd).unwrap().files[0].additions, 1);
    f.store.undo("unborn", cwd, None).unwrap();
    assert!(!unborn.join("new.txt").exists());
}
