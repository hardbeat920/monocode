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
