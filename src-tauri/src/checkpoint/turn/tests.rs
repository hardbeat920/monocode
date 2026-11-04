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
fn continuing_an_unfinished_turn_preserves_its_baseline_and_edit_evidence() {
    let f = Fixture::new();
    f.store.begin_turn("session", f.cwd(), "first").unwrap();
    f.store.begin_turn("other", f.cwd(), "other").unwrap();
    f.write("a.txt", "session edit\n");
    f.store
        .capture("session", f.cwd(), &["a.txt".into()])
        .unwrap();
    f.write("foreign.txt", "other session\n");
    // The UI reloads before it can finish the first checkpoint, then continues.
    let reopened = CheckpointStore::new(f.dir.join("checkpoints"));
    reopened.begin_turn("session", f.cwd(), "continue").unwrap();
    reopened
        .finish_turn("session", f.cwd(), "continue")
        .unwrap();
    let status = reopened.status("session", f.cwd()).unwrap();
    assert_eq!(status.files.len(), 1);
    assert_eq!(status.files[0].relative, "a.txt");
    assert!(!status.files[0].undoable);
    let diff = reopened.file_diff("session", f.cwd(), "a.txt").unwrap();
    assert_eq!(diff.original, "original\n");
    assert_eq!(diff.current, "session edit\n");
}

#[test]
fn recovers_edits_when_an_older_version_replaced_an_unfinished_baseline() {
    for committed in [false, true] {
        let f = Fixture::new();
        f.store.begin_turn("session", f.cwd(), "first").unwrap();
        f.write("a.txt", "session edit\n");
        f.write("foreign.txt", "other session\n");
        // Simulate the old Continue path losing the active metadata, but
        // leaving its original Git-backed baseline reference intact.
        let mut review = read_review(&f.store.session_dir("session"))
            .unwrap()
            .unwrap();
        review.active = None;
        write_review(&f.store.session_dir("session"), &review).unwrap();
        if committed {
            run_git(&f.repo, &["add", "a.txt"]).unwrap();
            run_git(&f.repo, &["commit", "-m", "accept edit"]).unwrap();
        }
        f.store.begin_turn("session", f.cwd(), "continue").unwrap();
        f.store.finish_turn("session", f.cwd(), "continue").unwrap();
        assert!(f.store.status("session", f.cwd()).unwrap().files.is_empty());
        f.store
            .recover_orphaned_turn("session", f.cwd(), &["removed-fixture.txt".into()])
            .unwrap();
        assert!(f.store.status("session", f.cwd()).unwrap().files.is_empty());
        f.store
            .recover_orphaned_turn("session", f.cwd(), &["a.txt".into()])
            .unwrap();
        let reopened = CheckpointStore::new(f.dir.join("checkpoints"));
        let status = reopened.status("session", f.cwd()).unwrap();
        if committed {
            assert!(status.files.is_empty());
        } else {
            assert_eq!(status.files.len(), 1);
            assert_eq!(status.files[0].relative, "a.txt");
            assert!(!status.files[0].undoable);
            let diff = reopened.file_diff("session", f.cwd(), "a.txt").unwrap();
            assert_eq!(diff.original, "original\n");
            assert_eq!(diff.current, "session edit\n");
        }
    }
}

#[test]
fn overlapping_turns_keep_exact_diffs_but_cannot_restore_shared_work() {
    let f = Fixture::new();
    f.write("a.txt", "top\n\nbottom\n");
    f.store.begin_turn("a", f.cwd(), "first").unwrap();
    f.store.begin_turn("b", f.cwd(), "second").unwrap();
    f.write("a.txt", "top edited by a\n\nbottom edited by b\n");
    for id in ["a", "b"] {
        f.store.capture(id, f.cwd(), &["a.txt".into()]).unwrap();
    }
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
fn shared_turns_only_review_files_with_their_own_successful_edit_events() {
    let f = Fixture::new();
    f.store.begin_turn("reader", f.cwd(), "first").unwrap();
    f.store.begin_turn("writer", f.cwd(), "second").unwrap();
    f.write("a.txt", "edited by the writer\n");
    f.store
        .capture("writer", f.cwd(), &["a.txt".into()])
        .unwrap();
    // A prepared/failed edit is not evidence that the reader changed a file.
    f.store
        .prepare("reader", f.cwd(), &["a.txt".into()])
        .unwrap();
    // The reader's temporary test fixture is removed before it finishes.
    f.write("temporary.txt", "temporary test\n");
    f.store
        .capture("reader", f.cwd(), &["temporary.txt".into()])
        .unwrap();
    std::fs::remove_file(f.repo.join("temporary.txt")).unwrap();
    f.store.finish_turn("reader", f.cwd(), "first").unwrap();
    f.store.finish_turn("writer", f.cwd(), "second").unwrap();

    let reopened = CheckpointStore::new(f.dir.join("checkpoints"));
    assert!(reopened.status("reader", f.cwd()).unwrap().files.is_empty());
    let status = reopened.status("writer", f.cwd()).unwrap();
    assert_eq!(status.files.len(), 1);
    assert_eq!(status.files[0].relative, "a.txt");
    assert!(!status.files[0].undoable);
    // The complete workspace recording is retained separately from attribution.
    assert_eq!(
        reopened
            .file_diff("reader", f.cwd(), "a.txt")
            .unwrap()
            .current,
        "edited by the writer\n"
    );
}

#[test]
fn shared_turns_with_different_edits_do_not_claim_each_others_files() {
    let f = Fixture::new();
    f.store.begin_turn("a", f.cwd(), "first").unwrap();
    f.store.begin_turn("b", f.cwd(), "second").unwrap();
    f.write("a.txt", "edited by a\n");
    // Windows strips trailing spaces from file names, so only the leading
    // space can be exercised there.
    let literal = if cfg!(windows) {
        " literal name .txt"
    } else {
        " literal name .txt "
    };
    f.write(literal, "edited by b\n");
    f.store.capture("a", f.cwd(), &["a.txt".into()]).unwrap();
    f.store
        .capture(
            "b",
            f.cwd(),
            &[f.repo.join(literal).to_string_lossy().into()],
        )
        .unwrap();
    f.store.finish_turn("a", f.cwd(), "first").unwrap();
    f.store.finish_turn("b", f.cwd(), "second").unwrap();
    for (id, relative) in [("a", "a.txt"), ("b", literal)] {
        let status = f.store.status(id, f.cwd()).unwrap();
        assert_eq!(status.files.len(), 1);
        assert_eq!(status.files[0].relative, relative);
    }
}

#[test]
fn old_shared_recordings_without_edit_evidence_do_not_claim_workspace_changes() {
    let f = Fixture::new();
    f.store.begin_turn("reader", f.cwd(), "first").unwrap();
    f.store.begin_turn("writer", f.cwd(), "second").unwrap();
    f.write("a.txt", "foreign change\n");
    f.store.finish_turn("reader", f.cwd(), "first").unwrap();
    let path = f.dir.join("checkpoints/reader/turn-review.json");
    let mut review: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    review["latest"].as_object_mut().unwrap().remove("edited");
    std::fs::write(path, serde_json::to_vec(&review).unwrap()).unwrap();
    let reopened = CheckpointStore::new(f.dir.join("checkpoints"));
    assert!(reopened.status("reader", f.cwd()).unwrap().files.is_empty());
}

#[test]
fn nested_turns_share_work_but_sibling_path_prefixes_do_not() {
    let f = Fixture::new();
    f.write("nested/a.txt", "child baseline\n");
    f.write("nested-other/a.txt", "sibling baseline\n");
    let child = f.repo.join("nested");
    let sibling = f.repo.join("nested-other");
    let child_cwd = child.to_str().unwrap();
    let sibling_cwd = sibling.to_str().unwrap();

    f.store.begin_turn("child", child_cwd, "first").unwrap();
    f.store.begin_turn("sibling", sibling_cwd, "first").unwrap();
    f.write("nested/a.txt", "independent child change\n");
    f.store.finish_turn("child", child_cwd, "first").unwrap();
    let status = f.store.status("child", child_cwd).unwrap();
    assert_eq!(status.files.len(), 1);
    assert!(status.files[0].undoable);
    f.store
        .finish_turn("sibling", sibling_cwd, "first")
        .unwrap();

    f.store.begin_turn("parent", f.cwd(), "second").unwrap();
    f.store.begin_turn("child", child_cwd, "second").unwrap();
    f.write("nested/a.txt", "shared parent and child change\n");
    f.store
        .capture("parent", f.cwd(), &["nested/a.txt".into()])
        .unwrap();
    f.store
        .capture("child", child_cwd, &["a.txt".into()])
        .unwrap();
    f.store.finish_turn("parent", f.cwd(), "second").unwrap();
    f.store.finish_turn("child", child_cwd, "second").unwrap();
    for (id, cwd) in [("parent", f.cwd()), ("child", child_cwd)] {
        let status = f.store.status(id, cwd).unwrap();
        assert_eq!(status.files.len(), 1);
        assert!(status.files[0].exact);
        assert!(!status.files[0].undoable);
        assert!(f.store.undo(id, cwd, None).is_err());
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
fn commits_during_the_turn_clear_review_but_preserve_the_recorded_diff() {
    let f = Fixture::new();
    f.store.begin_turn("session", f.cwd(), "first").unwrap();
    f.write("a.txt", "committed during the turn\n");
    run_git(&f.repo, &["add", "."]).unwrap();
    run_git(&f.repo, &["commit", "-m", "agent changes"]).unwrap();
    f.store.finish_turn("session", f.cwd(), "first").unwrap();
    assert!(run_git(&f.repo, &["status", "--porcelain"])
        .unwrap()
        .is_empty());
    assert!(f.store.status("session", f.cwd()).unwrap().files.is_empty());
    let diff = f.store.file_diff("session", f.cwd(), "a.txt").unwrap();
    assert_eq!(diff.original, "original\n");
    assert_eq!(diff.current, "committed during the turn\n");
}

#[test]
fn commits_after_the_turn_clear_modified_created_and_deleted_files() {
    let f = Fixture::new();
    f.store.begin_turn("session", f.cwd(), "first").unwrap();
    f.write("a.txt", "agent change\n");
    f.write("new.txt", "new file\n");
    std::fs::remove_file(f.repo.join("delete.txt")).unwrap();
    f.store.finish_turn("session", f.cwd(), "first").unwrap();
    assert_eq!(f.store.status("session", f.cwd()).unwrap().files.len(), 3);

    // The commit can include further edits made after the recorded turn.
    f.write("a.txt", "agent change\nuser addition\n");
    run_git(&f.repo, &["add", "-A"]).unwrap();
    run_git(&f.repo, &["commit", "-m", "accept changes"]).unwrap();
    assert!(f.store.status("session", f.cwd()).unwrap().files.is_empty());
    assert_eq!(
        f.store
            .file_diff("session", f.cwd(), "a.txt")
            .unwrap()
            .current,
        "agent change\n"
    );
    assert!(f.dir.join("checkpoints/session/turns/first.json").exists());
}

#[test]
fn unrelated_and_partial_commits_preserve_pending_files_and_exact_counts() {
    let f = Fixture::new();
    f.store.begin_turn("session", f.cwd(), "first").unwrap();
    f.write("a.txt", "agent change\n");
    f.write("new.txt", "new file\n");
    f.store.finish_turn("session", f.cwd(), "first").unwrap();

    f.write("unrelated.txt", "other work\n");
    run_git(&f.repo, &["add", "unrelated.txt"]).unwrap();
    run_git(&f.repo, &["commit", "-m", "unrelated work"]).unwrap();
    assert_eq!(f.store.status("session", f.cwd()).unwrap().files.len(), 2);

    run_git(&f.repo, &["add", "a.txt"]).unwrap();
    run_git(&f.repo, &["commit", "-m", "accept one file"]).unwrap();
    let status = f.store.status("session", f.cwd()).unwrap();
    assert_eq!(status.files.len(), 1);
    assert_eq!(status.files[0].relative, "new.txt");
    assert_eq!(
        (status.files[0].additions, status.files[0].deletions),
        (1, 0)
    );
    assert!(status.files[0].exact);
    assert!(!status.files[0].undoable);
}

#[test]
fn committing_only_part_of_a_files_changes_keeps_it_pending() {
    let f = Fixture::new();
    f.store.begin_turn("session", f.cwd(), "first").unwrap();
    f.write("a.txt", "first change\nsecond change\n");
    f.store.finish_turn("session", f.cwd(), "first").unwrap();
    f.write("a.txt", "first change\n");
    run_git(&f.repo, &["add", "a.txt"]).unwrap();
    run_git(&f.repo, &["commit", "-m", "accept part of the file"]).unwrap();
    f.write("a.txt", "first change\nsecond change\n");
    let status = f.store.status("session", f.cwd()).unwrap();
    assert_eq!(status.files.len(), 1);
    assert_eq!(status.files[0].relative, "a.txt");
    assert_eq!(
        (status.files[0].additions, status.files[0].deletions),
        (2, 1)
    );
}

#[test]
fn committed_results_do_not_reappear_after_later_edits_or_restarts() {
    let f = Fixture::new();
    f.store.begin_turn("session", f.cwd(), "first").unwrap();
    f.write("a.txt", "agent change\n");
    f.store.finish_turn("session", f.cwd(), "first").unwrap();
    run_git(&f.repo, &["add", "a.txt"]).unwrap();
    run_git(&f.repo, &["commit", "-m", "accept changes"]).unwrap();
    // A later edit must not keep the already committed turn's card alive.
    f.write("a.txt", "later editor change\n");
    assert!(f.store.status("session", f.cwd()).unwrap().files.is_empty());

    run_git(&f.repo, &["add", "a.txt"]).unwrap();
    run_git(&f.repo, &["commit", "-m", "later work"]).unwrap();
    f.write("a.txt", "another editor change\n");
    let reopened = CheckpointStore::new(f.dir.join("checkpoints"));
    assert!(reopened
        .status("session", f.cwd())
        .unwrap()
        .files
        .is_empty());
    assert_eq!(
        reopened
            .file_diff("session", f.cwd(), "a.txt")
            .unwrap()
            .current,
        "agent change\n"
    );
}

#[test]
fn commits_clear_review_in_nested_workspaces_and_repositories_without_head() {
    let f = Fixture::new();
    let nested = f.repo.join("nested");
    let unborn = f.dir.join("unborn");
    std::fs::create_dir(&nested).unwrap();
    std::fs::create_dir(&unborn).unwrap();
    run_git(&unborn, &["init"]).unwrap();
    run_git(&unborn, &["config", "user.name", "Test"]).unwrap();
    run_git(&unborn, &["config", "user.email", "test@example.invalid"]).unwrap();

    for (session, root) in [("nested", nested), ("unborn", unborn)] {
        let cwd = root.to_str().unwrap();
        let pending = "café pending.txt";
        f.store.begin_turn(session, cwd, "first").unwrap();
        std::fs::write(root.join("committed.txt"), "accepted\n").unwrap();
        std::fs::write(root.join(pending), "pending\n").unwrap();
        f.store.finish_turn(session, cwd, "first").unwrap();
        run_git(&root, &["add", "committed.txt"]).unwrap();
        run_git(&root, &["commit", "-m", "accept one file"]).unwrap();
        let status = f.store.status(session, cwd).unwrap();
        assert_eq!(status.files.len(), 1);
        assert_eq!(status.files[0].relative, pending);
        run_git(&root, &["add", pending]).unwrap();
        run_git(&root, &["commit", "-m", "accept remaining work"]).unwrap();
        assert!(f.store.status(session, cwd).unwrap().files.is_empty());
    }
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
    // Each worktree caches its own stat data; sharing one index would let the
    // worktrees overwrite each other's entries.
    let index_for = |path: &Path| {
        let indexes = f.store.index_dir().unwrap();
        let key = index_key(&std::fs::canonicalize(path).unwrap());
        let generation = index_generation(&indexes, &key).unwrap();
        index_file(&indexes, &key, generation)
    };
    assert_ne!(index_for(&f.repo), index_for(&linked));
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

fn private_index(f: &Fixture) -> PathBuf {
    let repo = std::fs::canonicalize(&f.repo).unwrap();
    let indexes = f.store.index_dir().unwrap();
    let key = index_key(&repo);
    let generation = index_generation(&indexes, &key).unwrap();
    index_file(&indexes, &key, generation)
}

/// The guard is per worktree, so its path must not follow the generation.
fn private_guard(f: &Fixture) -> PathBuf {
    let repo = std::fs::canonicalize(&f.repo).unwrap();
    f.store
        .index_dir()
        .unwrap()
        .join(format!("{}.capture", index_key(&repo)))
}

/// Git writes the index through a sibling lock file: `x.index.lock`.
fn index_lock(index: &Path) -> PathBuf {
    let mut lock = index.as_os_str().to_owned();
    lock.push(".lock");
    PathBuf::from(lock)
}

fn git_in_index(repo: &Path, index: &Path, args: &[&str]) -> Vec<u8> {
    let output = git_command(repo, args)
        .env("GIT_INDEX_FILE", index)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

fn index_paths(repo: &Path, index: &Path) -> Vec<String> {
    git_in_index(repo, index, &["ls-files", "-z", "--full-name"])
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| String::from_utf8(path.to_vec()).unwrap())
        .collect()
}

fn set_past_mtime(path: &Path) {
    let modified = std::time::SystemTime::now() - std::time::Duration::from_secs(4 * 60 * 60);
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(modified))
        .unwrap();
}

#[cfg(unix)]
fn count_clean_filter(repo: &Path, counter: &Path) {
    let clean = format!("sh -c 'echo tick >> {}; cat'", counter.display());
    run_git(repo, &["config", "filter.count.clean", &clean]).unwrap();
    run_git(repo, &["config", "filter.count.required", "true"]).unwrap();
    std::fs::write(repo.join(".gitattributes"), "*.big filter=count\n").unwrap();
}

#[cfg(unix)]
fn ticks(counter: &Path) -> usize {
    std::fs::read_to_string(counter)
        .map(|text| text.lines().count())
        .unwrap_or(0)
}

#[cfg(unix)]
#[test]
fn reuses_one_private_index_and_its_stat_cache_across_turns() {
    let f = Fixture::new();
    let counter = f.dir.join("clean-count");
    count_clean_filter(&f.repo, &counter);
    f.write("big.big", &"a".repeat(200_000));
    set_past_mtime(&f.repo.join("big.big"));
    f.store.begin_turn("session", f.cwd(), "first").unwrap();
    f.store.finish_turn("session", f.cwd(), "first").unwrap();
    let index = private_index(&f);
    assert!(index.exists());
    assert_eq!(ticks(&counter), 1);

    // The same store and a reopened store both reuse the index, so an
    // unchanged file is never hashed again.
    f.store.begin_turn("session", f.cwd(), "second").unwrap();
    f.store.finish_turn("session", f.cwd(), "second").unwrap();
    assert_eq!(private_index(&f), index);
    assert!(index.exists());
    assert_eq!(ticks(&counter), 1);
    let reopened = CheckpointStore::new(f.dir.join("checkpoints"));
    reopened.begin_turn("session", f.cwd(), "third").unwrap();
    reopened.finish_turn("session", f.cwd(), "third").unwrap();
    assert_eq!(ticks(&counter), 1);

    // A modified file must be hashed again, and the result must stay exact.
    reopened.begin_turn("session", f.cwd(), "fourth").unwrap();
    f.write("big.big", &"b".repeat(200_001));
    reopened.finish_turn("session", f.cwd(), "fourth").unwrap();
    assert!(ticks(&counter) > 1);
    assert_eq!(
        reopened
            .file_diff("session", f.cwd(), "big.big")
            .unwrap()
            .current,
        "b".repeat(200_001)
    );
}

#[test]
fn records_committed_files_that_the_ignore_rules_match() {
    let f = Fixture::new();
    f.write(".gitignore", "*.log\n");
    run_git(&f.repo, &["add", ".gitignore"]).unwrap();
    run_git(&f.repo, &["commit", "-m", "ignore logs"]).unwrap();
    f.write("debug.log", "first\n");
    // The private index is built while the file is untracked, so it starts
    // without the entry Git will later track through a forced commit.
    f.store.begin_turn("session", f.cwd(), "first").unwrap();
    f.store.finish_turn("session", f.cwd(), "first").unwrap();
    assert!(f.store.status("session", f.cwd()).unwrap().files.is_empty());

    run_git(&f.repo, &["add", "-f", "debug.log"]).unwrap();
    run_git(&f.repo, &["commit", "-m", "track the log"]).unwrap();
    f.store.begin_turn("session", f.cwd(), "second").unwrap();
    f.write("debug.log", "first\nsecond\n");
    f.store.finish_turn("session", f.cwd(), "second").unwrap();
    let status = f.store.status("session", f.cwd()).unwrap();
    assert_eq!(status.files.len(), 1);
    assert_eq!(status.files[0].relative, "debug.log");
    assert_eq!(
        (status.files[0].additions, status.files[0].deletions),
        (1, 0)
    );
    let diff = f.store.file_diff("session", f.cwd(), "debug.log").unwrap();
    assert_eq!(diff.original, "first\n");
    assert_eq!(diff.current, "first\nsecond\n");
}

#[cfg(unix)]
#[test]
fn head_advances_and_branch_switches_reuse_the_index() {
    let f = Fixture::new();
    let counter = f.dir.join("clean-count");
    count_clean_filter(&f.repo, &counter);
    f.write("first.big", &"s".repeat(1000));
    f.write("second.big", &"t".repeat(1000));
    set_past_mtime(&f.repo.join("first.big"));
    set_past_mtime(&f.repo.join("second.big"));
    f.store.begin_turn("session", f.cwd(), "first").unwrap();
    f.store.finish_turn("session", f.cwd(), "first").unwrap();
    assert_eq!(ticks(&counter), 2);

    // HEAD advances. A rebuilt index would hash the untouched files again.
    f.write("a.txt", "committed on main\n");
    run_git(&f.repo, &["add", "a.txt"]).unwrap();
    run_git(&f.repo, &["commit", "-m", "commit one file"]).unwrap();
    f.store.begin_turn("session", f.cwd(), "second").unwrap();
    f.write("a.txt", "committed on main\nsecond\n");
    f.store.finish_turn("session", f.cwd(), "second").unwrap();
    assert_eq!(ticks(&counter), 2);
    let diff = f.store.file_diff("session", f.cwd(), "a.txt").unwrap();
    assert_eq!(diff.original, "committed on main\n");
    assert_eq!(diff.current, "committed on main\nsecond\n");

    // A branch switch moves HEAD without invalidating the cached entries.
    run_git(&f.repo, &["checkout", "-b", "side"]).unwrap();
    f.store.begin_turn("session", f.cwd(), "third").unwrap();
    f.write("a.txt", "on the side branch\n");
    f.store.finish_turn("session", f.cwd(), "third").unwrap();
    assert_eq!(ticks(&counter), 2);
    let diff = f.store.file_diff("session", f.cwd(), "a.txt").unwrap();
    assert_eq!(diff.original, "committed on main\nsecond\n");
    assert_eq!(diff.current, "on the side branch\n");
}

#[cfg(unix)]
#[test]
fn an_unborn_repository_keeps_its_recording_after_the_first_commit() {
    let f = Fixture::new();
    let unborn = f.dir.join("unborn");
    std::fs::create_dir(&unborn).unwrap();
    run_git(&unborn, &["init"]).unwrap();
    run_git(&unborn, &["config", "user.name", "Test"]).unwrap();
    run_git(&unborn, &["config", "user.email", "test@example.invalid"]).unwrap();
    let counter = f.dir.join("unborn-count");
    count_clean_filter(&unborn, &counter);
    std::fs::write(unborn.join("keep.big"), "s".repeat(1000)).unwrap();
    set_past_mtime(&unborn.join("keep.big"));
    let cwd = unborn.to_str().unwrap();
    f.store.begin_turn("unborn", cwd, "first").unwrap();
    std::fs::write(unborn.join("first.txt"), "one\n").unwrap();
    f.store.finish_turn("unborn", cwd, "first").unwrap();
    assert_eq!(ticks(&counter), 1);

    // The first commit creates HEAD after the index was built without one.
    run_git(&unborn, &["add", ".gitattributes", "first.txt"]).unwrap();
    run_git(&unborn, &["commit", "-m", "first commit"]).unwrap();
    f.store.begin_turn("unborn", cwd, "second").unwrap();
    std::fs::write(unborn.join("first.txt"), "one\ntwo\n").unwrap();
    f.store.finish_turn("unborn", cwd, "second").unwrap();
    assert_eq!(ticks(&counter), 1);
    assert_eq!(
        f.store.status("unborn", cwd).unwrap().files[0].relative,
        "first.txt"
    );
    let diff = f.store.file_diff("unborn", cwd, "first.txt").unwrap();
    assert_eq!(diff.original, "one\n");
    assert_eq!(diff.current, "one\ntwo\n");
}

#[test]
fn drops_entries_that_become_ignored_before_recording() {
    let f = Fixture::new();
    f.write("debris/log.txt", "first\n");
    f.store.begin_turn("session", f.cwd(), "first").unwrap();
    f.store.finish_turn("session", f.cwd(), "first").unwrap();
    let index = private_index(&f);
    assert!(index_paths(&f.repo, &index).contains(&"debris/log.txt".to_string()));

    // The sensitive bytes go into the file the index already captured. A new
    // path would be ignored by `add` either way, so it could not tell a prune
    // that runs before the add from one that runs after it.
    let secret = "SENSITIVE-4b0f8f0e\n";
    f.write(".gitignore", "debris/\n");
    f.write("debris/log.txt", secret);
    f.store.begin_turn("session", f.cwd(), "second").unwrap();
    f.store.finish_turn("session", f.cwd(), "second").unwrap();
    // The cached index loses the entry instead of being rotated away.
    assert_eq!(private_index(&f), index);
    assert!(!index_paths(&f.repo, &index).contains(&"debris/log.txt".to_string()));
    // Neither the captured-then-ignored file's new bytes nor anything else the
    // rules now exclude reaches the object database.
    let oid = String::from_utf8(
        run_git(
            &f.repo,
            &["hash-object", "--no-filters", "--", "debris/log.txt"],
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(f.repo.join("debris/log.txt")).unwrap(),
        secret
    );
    assert!(run_git(&f.repo, &["cat-file", "-e", oid.trim()]).is_err());
}

#[test]
fn untracked_nested_ignore_rules_also_drop_entries() {
    let f = Fixture::new();
    // A rule file that is itself ignored still applies. Written before the
    // baseline so the only thing that changes later is the nested rule, which
    // an `--exclude-standard` listing would hide.
    f.write(".gitignore", ".gitignore\n");
    f.write("nested/secret.txt", "visible\n");
    f.store.begin_turn("session", f.cwd(), "first").unwrap();
    f.store.finish_turn("session", f.cwd(), "first").unwrap();
    let index = private_index(&f);
    assert!(index_paths(&f.repo, &index).contains(&"nested/secret.txt".to_string()));

    f.write("nested/.gitignore", "secret.txt\n");
    let secret = "SENSITIVE-9c31ab77\n";
    f.write("nested/secret.txt", secret);
    f.store.begin_turn("session", f.cwd(), "second").unwrap();
    f.store.finish_turn("session", f.cwd(), "second").unwrap();
    assert_eq!(private_index(&f), index);
    assert!(!index_paths(&f.repo, &index).contains(&"nested/secret.txt".to_string()));
    let oid = String::from_utf8(
        run_git(
            &f.repo,
            &["hash-object", "--no-filters", "--", "nested/secret.txt"],
        )
        .unwrap(),
    )
    .unwrap();
    assert!(run_git(&f.repo, &["cat-file", "-e", oid.trim()]).is_err());
}

#[test]
fn drops_a_file_git_stopped_tracking_once_a_rule_covers_it() {
    let f = Fixture::new();
    f.write(".env", "TOKEN=old\n");
    run_git(&f.repo, &["add", "-f", ".env"]).unwrap();
    run_git(&f.repo, &["commit", "-m", "track env"]).unwrap();
    f.store.begin_turn("session", f.cwd(), "first").unwrap();
    f.store.finish_turn("session", f.cwd(), "first").unwrap();
    assert!(index_paths(&f.repo, &private_index(&f)).contains(&".env".to_string()));

    // The rules never change again: the cached entry outlives the tracking.
    f.write(".gitignore", ".env\n");
    run_git(&f.repo, &["rm", "--cached", ".env"]).unwrap();
    run_git(&f.repo, &["commit", "-m", "stop tracking env"]).unwrap();
    let secret = "TOKEN=SENSITIVE-1d5c7a20\n";
    f.write(".env", secret);
    f.store.begin_turn("session", f.cwd(), "second").unwrap();
    f.store.finish_turn("session", f.cwd(), "second").unwrap();
    assert!(!index_paths(&f.repo, &private_index(&f)).contains(&".env".to_string()));
    let oid = String::from_utf8(
        run_git(&f.repo, &["hash-object", "--no-filters", "--", ".env"]).unwrap(),
    )
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(f.repo.join(".env")).unwrap(),
        secret
    );
    assert!(run_git(&f.repo, &["cat-file", "-e", oid.trim()]).is_err());
}

#[test]
fn rebuilds_a_missing_or_truncated_private_index_once() {
    let f = Fixture::new();
    let head = run_git(&f.repo, &["rev-parse", "HEAD"]).unwrap();
    let real_index = std::fs::read(f.repo.join(".git/index")).unwrap();
    f.store.begin_turn("session", f.cwd(), "first").unwrap();
    f.write("new.txt", "one\n");
    f.store.finish_turn("session", f.cwd(), "first").unwrap();
    let index = private_index(&f);

    std::fs::remove_file(&index).unwrap();
    f.store.begin_turn("session", f.cwd(), "second").unwrap();
    f.write("new.txt", "one\ntwo\n");
    f.store.finish_turn("session", f.cwd(), "second").unwrap();
    assert_eq!(
        f.store
            .file_diff("session", f.cwd(), "new.txt")
            .unwrap()
            .current,
        "one\ntwo\n"
    );

    std::fs::write(&index, b"DIRC").unwrap();
    f.store.begin_turn("session", f.cwd(), "third").unwrap();
    f.write("new.txt", "one\ntwo\nthree\n");
    f.store.finish_turn("session", f.cwd(), "third").unwrap();
    let diff = f.store.file_diff("session", f.cwd(), "new.txt").unwrap();
    assert_eq!(diff.original, "one\ntwo\n");
    assert_eq!(diff.current, "one\ntwo\nthree\n");

    assert_eq!(
        std::fs::read(f.repo.join(".git/index")).unwrap(),
        real_index
    );
    assert_eq!(run_git(&f.repo, &["rev-parse", "HEAD"]).unwrap(), head);
}

#[cfg(unix)]
#[test]
fn records_nested_unusual_paths_and_symlinks_with_a_persistent_index() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let f = Fixture::new();
    run_git(&f.repo, &["config", "core.fileMode", "true"]).unwrap();
    let nested = f.repo.join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    f.write("nested/normal.txt", "baseline\n");
    f.write("nested/mode.sh", "#!/bin/sh\n");
    run_git(&f.repo, &["add", "nested"]).unwrap();
    run_git(&f.repo, &["commit", "-m", "nested baseline"]).unwrap();
    let cwd = nested.to_str().unwrap();
    let unusual = "tab\tand 中文 name.txt";
    f.store.begin_turn("session", cwd, "first").unwrap();
    f.write(&format!("nested/{unusual}"), "new\n");
    std::fs::remove_file(nested.join("normal.txt")).unwrap();
    std::fs::set_permissions(
        nested.join("mode.sh"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    symlink("mode.sh", nested.join("link.txt")).unwrap();
    f.store.finish_turn("session", cwd, "first").unwrap();
    let status = f.store.status("session", cwd).unwrap();
    assert_eq!(
        status
            .files
            .iter()
            .map(|file| file.relative.as_str())
            .collect::<Vec<_>>(),
        ["link.txt", "mode.sh", "normal.txt", unusual]
    );
    assert_eq!(
        f.store.file_diff("session", cwd, unusual).unwrap().current,
        "new\n"
    );
    assert!(
        status
            .files
            .iter()
            .find(|file| file.relative == "link.txt")
            .unwrap()
            .exact
    );
}

#[test]
fn a_leftover_lock_heals_under_the_capture_guard() {
    let f = Fixture::new();
    f.store.begin_turn("session", f.cwd(), "first").unwrap();
    f.store.finish_turn("session", f.cwd(), "first").unwrap();
    let index = private_index(&f);
    let lock = index_lock(&index);
    // A capture that died mid-write leaves its lock behind. The guard its
    // process held is gone, so the next capture takes a fresh generation
    // immediately instead of waiting for the lock to look old.
    std::fs::write(&lock, b"").unwrap();
    f.store.begin_turn("session", f.cwd(), "second").unwrap();
    assert_ne!(
        private_index(&f),
        index,
        "the lock must move the capture on"
    );
    f.write("new.txt", "one\n");
    f.store.finish_turn("session", f.cwd(), "second").unwrap();
    let status = f.store.status("session", f.cwd()).unwrap();
    assert_eq!(
        status
            .files
            .iter()
            .find(|file| file.relative == "new.txt")
            .map(|file| file.additions),
        Some(1)
    );
}

#[test]
fn concurrent_captures_of_one_workspace_serialize() {
    let f = Fixture::new();
    f.write("new.txt", "one\n");
    let repo = std::fs::canonicalize(&f.repo).unwrap();
    let indexes = f.store.index_dir().unwrap();
    let captures = (0..2)
        .map(|n| {
            let repo = repo.clone();
            let indexes = indexes.clone();
            std::thread::spawn(move || {
                capture_workspace(&repo, &repo, &format!("refs/monocode/test/{n}"), &indexes)
            })
        })
        .collect::<Vec<_>>();
    let results = captures
        .into_iter()
        .map(|capture| capture.join().unwrap())
        .collect::<Vec<_>>();
    assert!(results.iter().all(|result| result.is_ok()), "{results:?}");
    // Both saw the same workspace, so neither read the other's half-written
    // index, and the index they share survived both captures.
    let trees = results
        .iter()
        .map(|commit| {
            let commit = commit.as_ref().unwrap();
            let tree = run_git(&repo, &["rev-parse", &format!("{commit}^{{tree}}")]).unwrap();
            String::from_utf8(tree).unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(trees[0], trees[1]);
    assert!(index_paths(&repo, &private_index(&f)).contains(&"new.txt".to_string()));
}

#[test]
fn a_capture_guard_held_elsewhere_times_out() {
    let f = Fixture::new();
    let guard = private_guard(&f);
    let held = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&guard)
        .unwrap();
    held.try_lock().unwrap();
    let error = CaptureGuard::acquire_within(&guard, Duration::from_millis(50))
        .err()
        .expect("a guard another owner holds must not be taken twice");
    assert!(error.contains("Another MonoCode instance"), "{error}");
}

#[cfg(unix)]
#[test]
fn a_file_named_index_does_not_rebuild_the_index() {
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    f.write("index.html", "<html>\n");
    f.store.begin_turn("session", f.cwd(), "first").unwrap();
    f.store.finish_turn("session", f.cwd(), "first").unwrap();
    let index = private_index(&f);
    let inode = std::fs::metadata(&index).unwrap().ino();

    let file = f.repo.join("index.html");
    std::fs::write(&file, "<html>two\n").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read(&file).is_ok() {
        // Root ignores file modes, so the failure cannot be provoked here.
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        return;
    }
    let error = f
        .store
        .begin_turn("session", f.cwd(), "second")
        .unwrap_err();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(error.contains("index.html"), "{error}");
    // Rebuilding would replace the file and lose the stat cache.
    assert_eq!(std::fs::metadata(&index).unwrap().ino(), inode);
}

/// A `git` child of a killed app renames its lock by path. Abandoning the
/// index name is what keeps it away from the capture that follows.
#[test]
fn a_dead_writer_cannot_reach_the_next_generation() {
    let f = Fixture::new();
    f.write("a.txt", "one\n");
    f.store.begin_turn("session", f.cwd(), "first").unwrap();
    f.store.finish_turn("session", f.cwd(), "first").unwrap();
    let dead = private_index(&f);
    let lock = index_lock(&dead);
    std::fs::write(&lock, b"").unwrap();

    f.store.begin_turn("session", f.cwd(), "second").unwrap();
    f.write("a.txt", "two\n");
    f.store.finish_turn("session", f.cwd(), "second").unwrap();
    let live = private_index(&f);
    assert_ne!(live, dead);
    // The dead writer's lock is left alone: the capture moved to a path the
    // dead writer never saw.
    assert!(lock.exists());

    // Whatever the dead writer does with its own path ...
    std::fs::write(&lock, b"partial").unwrap();
    std::fs::rename(&lock, &dead).unwrap();
    // ... the current generation and its recording are untouched.
    assert!(live.exists());
    assert!(std::fs::read(&live).unwrap().starts_with(b"DIRC"));
    assert_eq!(
        f.store
            .file_diff("session", f.cwd(), "a.txt")
            .unwrap()
            .current,
        "two\n"
    );
}
