use super::*;
use std::path::PathBuf;

fn tmp() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

fn git(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .args([
            "-c",
            "user.name=MonoCode",
            "-c",
            "user.email=monocode@test",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "MonoCode")
        .env("GIT_AUTHOR_EMAIL", "monocode@test")
        .env("GIT_COMMITTER_NAME", "MonoCode")
        .env("GIT_COMMITTER_EMAIL", "monocode@test")
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn init_git(dir: &Path, branch: &str) -> bool {
    git(dir, &["init"])
        && git(
            dir,
            &["symbolic-ref", "HEAD", &format!("refs/heads/{branch}")],
        )
        && git(dir, &["config", "user.name", "MonoCode"])
        && git(dir, &["config", "user.email", "monocode@test"])
        && git(dir, &["config", "commit.gpgsign", "false"])
        && git(dir, &["config", "core.autocrlf", "false"])
}

fn init_git_commit(dir: &Path, files: &[(&str, &str)]) -> bool {
    if !init_git(dir, "main") {
        return false;
    }
    for (name, contents) in files {
        if std::fs::write(dir.join(name), contents).is_err() {
            return false;
        }
    }
    git(dir, &["add", "."]) && git(dir, &["commit", "-m", "init"])
}

fn init_bare(dir: &Path) -> bool {
    git(dir, &["init", "--bare"])
}

fn index(dir: &Path) -> DiffIndex {
    diff_index(dir).unwrap()
}

fn file(dir: &Path, relative: &str) -> ChangedFile {
    index(dir)
        .files
        .into_iter()
        .find(|file| file.relative == relative)
        .unwrap()
}

#[test]
fn review_versions_invalidate_only_changed_file_sides() {
    let dir = tmp();
    let root = dir.path();
    assert!(init_git_commit(
        root,
        &[("a.txt", "base\n"), ("b.txt", "base\n")]
    ));
    std::fs::write(root.join("a.txt"), "stage\n").unwrap();
    assert!(git(root, &["add", "a.txt"]));
    std::fs::write(root.join("a.txt"), "work1\n").unwrap();
    std::fs::write(root.join("b.txt"), "other\n").unwrap();
    let first = diff_files(root).unwrap();
    let versions = file_diff_versions(root, &first).unwrap();
    assert_eq!(versions, file_diff_versions(root, &first).unwrap());

    std::fs::write(root.join("a.txt"), "work2\n").unwrap();
    let second = diff_files(root).unwrap();
    assert_eq!(first, second, "line counts cannot detect this edit");
    let edited = file_diff_versions(root, &second).unwrap();
    assert_ne!(versions["unstaged:a.txt"], edited["unstaged:a.txt"]);
    assert_eq!(versions["staged:a.txt"], edited["staged:a.txt"]);
    assert_eq!(versions["unstaged:b.txt"], edited["unstaged:b.txt"]);

    assert!(git(root, &["add", "a.txt"]));
    let staged = file_diff_versions(root, &second).unwrap();
    assert_ne!(edited["staged:a.txt"], staged["staged:a.txt"]);
    assert_ne!(edited["unstaged:a.txt"], staged["unstaged:a.txt"]);
    assert_eq!(edited["unstaged:b.txt"], staged["unstaged:b.txt"]);
}

#[test]
fn review_index_revalidates_closed_views_and_subdirectories() {
    let dir = tmp();
    let root = dir.path();
    std::fs::create_dir(root.join("src")).unwrap();
    assert!(init_git_commit(root, &[("src/a.txt", "base\n")]));
    std::fs::write(root.join("src/a.txt"), "edit\n").unwrap();
    let before = diff_files(root).unwrap();
    let added_name = if cfg!(windows) {
        " new file.txt"
    } else {
        " new\tfile.txt"
    };
    std::fs::write(root.join("src").join(added_name), "new\n").unwrap();
    let next = review_index(root, Some(&before)).unwrap();
    assert_eq!(next.files.len(), 2);
    assert_eq!(next.files[1].additions, before.files[0].additions);
    let sub = review_index(&root.join("src"), None).unwrap();
    assert_eq!(sub.files[0].relative, added_name);
    assert_eq!(sub.files[1].relative, "a.txt");
    let versions = file_diff_versions(&root.join("src"), &sub).unwrap();
    assert!(versions.contains_key(&format!("unstaged:{added_name}")));
    assert!(versions.contains_key("unstaged:a.txt"));
    std::fs::remove_file(root.join("src/a.txt")).unwrap();
    let deleted = review_index(root, None).unwrap();
    assert_eq!(deleted.files[1].status, "deleted");
    assert!(file_diff_versions(root, &deleted).is_ok());
}

#[test]
fn review_versions_support_repositories_with_only_untracked_files() {
    let dir = tmp();
    assert!(init_git(dir.path(), "main"));
    std::fs::write(dir.path().join("new.txt"), "new\n").unwrap();
    let snapshot = review_index(dir.path(), None).unwrap();
    assert!(file_diff_versions(dir.path(), &snapshot)
        .unwrap()
        .contains_key("unstaged:new.txt"));
}

#[test]
fn diff_stats_are_zero_outside_a_repo() {
    let dir = tmp();
    std::fs::write(dir.path().join("notes.txt"), "hello\n").unwrap();
    assert_eq!(diff_stats(dir.path()).unwrap(), DiffStats::default());
}

#[test]
fn diff_stats_count_unstaged_and_untracked() {
    let dir = tmp();
    let dir = dir.path();
    assert!(init_git_commit(dir, &[("a.txt", "alpha\nbeta\ngamma\n")]));
    std::fs::write(dir.join("a.txt"), "alpha\ngamma\ndelta\n").unwrap();
    std::fs::write(dir.join("new.txt"), "hello\nworld\n").unwrap();
    std::fs::write(dir.join("ignored.txt"), "nope\n").unwrap();
    std::fs::write(dir.join(".gitignore"), "ignored.txt\n").unwrap();
    // a.txt: -beta +delta; new.txt: +2; .gitignore: +1 untracked
    assert_eq!(
        diff_stats(dir).unwrap(),
        DiffStats {
            files: 3,
            additions: 4,
            deletions: 1
        }
    );
}

#[test]
fn diff_stats_are_zero_when_clean() {
    let dir = tmp();
    assert!(init_git_commit(dir.path(), &[("a.txt", "alpha\n")]));
    assert_eq!(diff_stats(dir.path()).unwrap(), DiffStats::default());
}

#[test]
fn diff_index_lists_modified_and_untracked() {
    let dir = tmp();
    let dir = dir.path();
    assert!(init_git_commit(dir, &[("a.txt", "alpha\nbeta\ngamma\n")]));
    std::fs::write(dir.join("a.txt"), "alpha\ngamma\ndelta\n").unwrap();
    std::fs::write(dir.join("new.txt"), "hello\nworld\n").unwrap();

    let index = index(dir);
    assert_eq!(index.branch.as_deref(), Some("main"));
    assert_eq!(index.files.len(), 2);
    assert_eq!((index.additions, index.deletions), (3, 1));
    let modified = &index.files[0];
    assert_eq!(modified.relative, "a.txt");
    assert_eq!(modified.status, "modified");
    assert_eq!((modified.additions, modified.deletions), (1, 1));
    assert_eq!(modified.path, path_to_js(&dir.join("a.txt")));
    let untracked = &index.files[1];
    assert_eq!(untracked.relative, "new.txt");
    assert_eq!(untracked.status, "untracked");
    assert_eq!((untracked.additions, untracked.deletions), (2, 0));
    assert!(untracked.unstaged && !untracked.staged);
}

#[test]
fn diff_index_reports_added_and_deleted() {
    let dir = tmp();
    let dir = dir.path();
    assert!(init_git_commit(
        dir,
        &[("a.txt", "alpha\n"), ("b.txt", "b\n")]
    ));
    std::fs::write(dir.join("added.txt"), "x\n").unwrap();
    stage_file(dir, "added.txt").unwrap();
    std::fs::remove_file(dir.join("b.txt")).unwrap();
    assert_eq!(file(dir, "added.txt").status, "added");
    assert_eq!(file(dir, "b.txt").status, "deleted");
}

#[test]
fn diff_index_keeps_literal_non_ascii_paths() {
    let dir = tmp();
    let dir = dir.path();
    assert!(init_git_commit(
        dir,
        &[("한글.md", "before\n"), ("my file.txt", "one\n")]
    ));
    assert!(git(dir, &["config", "core.quotePath", "true"]));
    std::fs::write(dir.join("한글.md"), "before\nafter\n").unwrap();
    std::fs::write(dir.join("my file.txt"), "one\ntwo\n").unwrap();

    let hangul = file(dir, "한글.md");
    assert_eq!(hangul.status, "modified");
    assert_eq!(hangul.additions, 1);
    assert!(hangul.unstaged);
    let spaced = file(dir, "my file.txt");
    assert_eq!(spaced.status, "modified");
    assert_eq!(spaced.additions, 1);
}

#[test]
fn diff_index_keeps_paths_relative_to_nested_workspace() {
    let dir = tmp();
    let dir = dir.path();
    let workspace = dir.join("sub");
    std::fs::create_dir_all(workspace.join("sub")).unwrap();
    assert!(init_git_commit(
        dir,
        &[("sub/file.txt", "original\n"), ("sub/second.txt", "old\n")],
    ));
    std::fs::write(workspace.join("file.txt"), "staged\n").unwrap();
    assert!(git(dir, &["add", "--", "sub/file.txt"]));
    std::fs::write(workspace.join("second.txt"), "unstaged\n").unwrap();
    // Without --relative this collides with the tracked sub/file.txt key.
    std::fs::write(workspace.join("sub/file.txt"), "untracked\n").unwrap();

    let changes = index(&workspace);
    assert_eq!(changes.files.len(), 3);
    for (relative, staged, unstaged) in [
        ("file.txt", true, false),
        ("second.txt", false, true),
        ("sub/file.txt", false, true),
    ] {
        let file = changes
            .files
            .iter()
            .find(|file| file.relative == relative)
            .unwrap();
        assert_eq!(file.path, path_to_js(&workspace.join(relative)));
        assert_eq!((file.staged, file.unstaged), (staged, unstaged));
    }
}

#[test]
fn diff_index_works_without_a_head() {
    let dir = tmp();
    let dir = dir.path();
    assert!(init_git(dir, "main"));
    std::fs::write(dir.join("a.txt"), "one\ntwo\n").unwrap();
    std::fs::write(dir.join("b.txt"), "x\n").unwrap();
    stage_file(dir, "a.txt").unwrap();
    let index = index(dir);
    assert_eq!(index.head, None);
    assert_eq!(index.branch.as_deref(), Some("main"));
    let a = &index.files[0];
    assert_eq!((a.relative.as_str(), a.status.as_str()), ("a.txt", "added"));
    assert_eq!(a.additions, 2);
    assert!(a.staged && !a.unstaged);
    assert_eq!(index.files[1].status, "untracked");
}

#[test]
fn parse_decorations_classifies_head_local_remote_and_tags() {
    let remotes = vec!["origin".to_string()];
    let (head, refs) = parse_decorations(
        "HEAD -> main, origin/main, origin/HEAD, tag: v0.1.30, fix/foo",
        Some("abc"),
        "abc",
        &remotes,
    );
    assert!(head);
    let refs: Vec<_> = refs
        .iter()
        .map(|r| (r.name.as_str(), r.kind.as_str()))
        .collect();
    assert_eq!(
        refs,
        [
            ("main", "local"),
            ("origin/main", "remote"),
            ("v0.1.30", "tag"),
            ("fix/foo", "local"),
        ]
    );
    let (head, refs) = parse_decorations("HEAD", None, "abc", &remotes);
    assert!(head && refs.is_empty());
    assert!(!parse_decorations("", Some("def"), "abc", &remotes).0);
}

#[test]
fn parse_history_log_reads_records() {
    let text = "aaaa\0aa\0bbbb cccc\0Ann\x001700000000\0HEAD -> main\0Merge it\x1e\n\
                bbbb\0\0\0Bob\0bad\0\0\x1e\nshort\0record\x1e";
    let commits = parse_history_log(text, Some("aaaa"), &[]);
    assert_eq!(commits.len(), 2);
    assert_eq!(commits[0].parents, ["bbbb", "cccc"]);
    assert_eq!(commits[0].timestamp, 1_700_000_000);
    assert_eq!(commits[0].subject, "Merge it");
    assert!(commits[0].head);
    assert_eq!(commits[1].short_sha, "bbbb");
    assert_eq!(commits[1].timestamp, 0);
    assert!(commits[1].parents.is_empty() && !commits[1].head);
}

#[test]
fn history_lists_commits_parents_and_head() {
    let dir = tmp();
    let dir = dir.path();
    assert!(init_git_commit(dir, &[("a.txt", "alpha\n")]));
    std::fs::write(dir.join("a.txt"), "beta\n").unwrap();
    assert!(git(dir, &["add", "."]));
    assert!(git(dir, &["commit", "-m", "second"]));

    let history = history(dir, Some(10)).unwrap();
    assert_eq!(history.commits.len(), 2);
    assert_eq!(
        history.head.as_deref(),
        Some(history.commits[0].sha.as_str())
    );
    assert_eq!(history.commits[0].subject, "second");
    assert_eq!(history.commits[1].subject, "init");
    assert_eq!(
        history.commits[0].parents,
        vec![history.commits[1].sha.clone()]
    );
    assert!(history.commits[0].head);
    assert!(!history.commits[1].head);
    assert_eq!(history.commits[0].author, "MonoCode");
    assert!(history.commits[0]
        .refs
        .iter()
        .any(|r| r.kind == "local" && r.name == "main"));
    assert_eq!(super::history(dir, Some(1)).unwrap().commits.len(), 1);
}

#[test]
fn history_empty_outside_a_repo() {
    let dir = tmp();
    assert_eq!(history(dir.path(), None).unwrap(), History::default());
}

#[test]
fn history_records_merge_parents() {
    let dir = tmp();
    let dir = dir.path();
    assert!(init_git_commit(dir, &[("a.txt", "alpha\n")]));
    assert!(git(dir, &["checkout", "-b", "feature"]));
    std::fs::write(dir.join("feat.txt"), "one\n").unwrap();
    assert!(git(dir, &["add", "."]));
    assert!(git(dir, &["commit", "-m", "feature work"]));
    assert!(git(dir, &["checkout", "main"]));
    std::fs::write(dir.join("main.txt"), "two\n").unwrap();
    assert!(git(dir, &["add", "."]));
    assert!(git(dir, &["commit", "-m", "main work"]));
    assert!(git(
        dir,
        &["merge", "feature", "--no-ff", "-m", "Merge feature"]
    ));
    let history = history(dir, Some(20)).unwrap();
    let merge = history
        .commits
        .iter()
        .find(|commit| commit.subject == "Merge feature")
        .unwrap();
    assert_eq!(merge.parents.len(), 2);
}

#[test]
fn history_skips_stash_and_unmerged_branches() {
    let dir = tmp();
    let dir = dir.path();
    assert!(init_git_commit(dir, &[("a.txt", "alpha\n")]));
    assert!(git(dir, &["checkout", "-b", "feature"]));
    std::fs::write(dir.join("feat.txt"), "one\n").unwrap();
    assert!(git(dir, &["add", "."]));
    assert!(git(dir, &["commit", "-m", "feature only"]));
    std::fs::write(dir.join("wip.txt"), "stash me\n").unwrap();
    assert!(git(dir, &["stash", "push", "-u", "-m", "wip stash"]));
    assert!(git(dir, &["checkout", "main"]));
    std::fs::write(dir.join("main.txt"), "two\n").unwrap();
    assert!(git(dir, &["add", "."]));
    assert!(git(dir, &["commit", "-m", "main only"]));

    let history = history(dir, Some(20)).unwrap();
    let subjects: Vec<&str> = history
        .commits
        .iter()
        .map(|commit| commit.subject.as_str())
        .collect();
    assert!(subjects.contains(&"main only"));
    assert!(subjects.contains(&"init"));
    assert!(!subjects.iter().any(|s| s.contains("feature only")));
    assert!(!subjects
        .iter()
        .any(|s| s.contains("wip stash") || s.contains("WIP on")));
}

#[test]
fn stage_and_unstage_file() {
    let dir = tmp();
    let dir = dir.path();
    assert!(init_git_commit(dir, &[("a.txt", "alpha\n")]));
    std::fs::write(dir.join("a.txt"), "beta\n").unwrap();
    stage_file(dir, "a.txt").unwrap();
    let staged = file(dir, "a.txt");
    assert!(staged.staged && !staged.unstaged);
    unstage_file(dir, "a.txt").unwrap();
    let unstaged = file(dir, "a.txt");
    assert!(!unstaged.staged && unstaged.unstaged);
}

#[test]
fn stage_and_unstage_directory() {
    let dir = tmp();
    let dir = dir.path();
    std::fs::create_dir_all(dir.join("src/nested")).unwrap();
    std::fs::create_dir_all(dir.join("src-other")).unwrap();
    assert!(init_git_commit(
        dir,
        &[
            ("src/app.ts", "before\n"),
            ("src/nested/deleted.ts", "delete me\n"),
            ("src-other/app.ts", "before\n"),
            ("ready.txt", "before\n"),
            (".gitignore", "src/ignored.txt\n"),
        ],
    ));
    std::fs::write(dir.join("src/app.ts"), "after\n").unwrap();
    std::fs::remove_dir_all(dir.join("src/nested")).unwrap();
    std::fs::create_dir_all(dir.join("src/added")).unwrap();
    std::fs::write(dir.join("src/added/new.ts"), "new\n").unwrap();
    std::fs::write(dir.join("src/ignored.txt"), "ignored\n").unwrap();
    std::fs::write(dir.join("src-other/app.ts"), "outside\n").unwrap();
    std::fs::write(dir.join("ready.txt"), "ready\n").unwrap();
    stage_file(dir, "ready.txt").unwrap();

    stage_file(dir, "src").unwrap();
    let staged = index(dir);
    assert_eq!(staged.files.len(), 5);
    for file in &staged.files {
        let in_folder = file.relative.starts_with("src/");
        assert_eq!(file.staged, in_folder || file.relative == "ready.txt");
        assert_eq!(file.unstaged, file.relative == "src-other/app.ts");
    }

    unstage_file(dir, "src").unwrap();
    let unstaged = index(dir);
    assert_eq!(unstaged.files.len(), 5);
    for file in &unstaged.files {
        assert_eq!(file.staged, file.relative == "ready.txt");
        assert_eq!(file.unstaged, file.relative != "ready.txt");
    }
    assert_eq!(
        std::fs::read_to_string(dir.join("src/app.ts")).unwrap(),
        "after\n"
    );
}

#[cfg(unix)]
#[test]
fn stage_and_unstage_literal_directory_names() {
    for folder in ["*", "folder?", "[ab]", ":(glob)*"] {
        let dir = tmp();
        let dir = dir.path();
        for directory in [folder, "a", "folderx"] {
            std::fs::create_dir(dir.join(directory)).unwrap();
        }
        let inside = format!("{folder}/inside.txt");
        let tracked = [
            inside.as_str(),
            "a/other.txt",
            "folderx/other.txt",
            "ready.txt",
        ];
        let initial: Vec<_> = tracked.iter().map(|path| (*path, "before\n")).collect();
        assert!(init_git_commit(dir, &initial));
        for path in tracked {
            std::fs::write(dir.join(path), "after\n").unwrap();
        }
        let staged_paths = || {
            git_run(dir, &["diff", "--cached", "--name-only", "-z"])
                .unwrap()
                .split('\0')
                .filter(|path| !path.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        };
        stage_file(dir, "ready.txt").unwrap();
        stage_file(dir, folder).unwrap();
        let mut expected = vec![inside.clone(), "ready.txt".to_string()];
        expected.sort();
        assert_eq!(staged_paths(), expected, "stage folder {folder}");
        unstage_file(dir, folder).unwrap();
        assert_eq!(staged_paths(), vec!["ready.txt"], "unstage folder {folder}");
    }
}

#[test]
fn stage_all_and_unstage_all() {
    let dir = tmp();
    let dir = dir.path();
    assert!(init_git_commit(
        dir,
        &[("a.txt", "alpha\n"), ("b.txt", "b\n")]
    ));
    std::fs::write(dir.join("a.txt"), "beta\n").unwrap();
    std::fs::remove_file(dir.join("b.txt")).unwrap();
    std::fs::write(dir.join("new.txt"), "hello\n").unwrap();
    stage_all(dir).unwrap();
    assert!(index(dir).files.iter().all(|f| f.staged && !f.unstaged));
    assert_eq!(file(dir, "new.txt").status, "added");
    unstage_all(dir).unwrap();
    assert!(index(dir).files.iter().all(|f| !f.staged && f.unstaged));
    assert_eq!(file(dir, "new.txt").status, "untracked");
}

#[test]
fn discard_restores_tracked_and_deletes_untracked() {
    let dir = tmp();
    let dir = dir.path();
    assert!(init_git_commit(dir, &[("a.txt", "alpha\n")]));
    std::fs::write(dir.join("a.txt"), "beta\n").unwrap();
    std::fs::write(dir.join("new.txt"), "hello\n").unwrap();
    std::fs::create_dir(dir.join("newdir")).unwrap();
    std::fs::write(dir.join("newdir/x.txt"), "x\n").unwrap();
    discard_file(dir, "a.txt").unwrap();
    discard_file(dir, "new.txt").unwrap();
    discard_file(dir, "newdir").unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.join("a.txt")).unwrap(),
        "alpha\n"
    );
    assert!(!dir.join("new.txt").exists());
    assert!(!dir.join("newdir/x.txt").exists());
    assert!(index(dir).files.is_empty());
}

#[test]
fn discard_all_restores_unstaged_and_keeps_staged() {
    let dir = tmp();
    let dir = dir.path();
    assert!(init_git_commit(
        dir,
        &[("a.txt", "alpha\n"), ("b.txt", "one\n")]
    ));
    std::fs::write(dir.join("a.txt"), "beta\n").unwrap();
    std::fs::write(dir.join("b.txt"), "two\n").unwrap();
    stage_file(dir, "b.txt").unwrap();
    std::fs::write(dir.join("b.txt"), "three\n").unwrap();
    std::fs::write(dir.join("new.txt"), "hello\n").unwrap();
    discard_all(dir).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.join("a.txt")).unwrap(),
        "alpha\n"
    );
    assert_eq!(std::fs::read_to_string(dir.join("b.txt")).unwrap(), "two\n");
    assert!(!dir.join("new.txt").exists());
    let b = file(dir, "b.txt");
    assert!(b.staged && !b.unstaged);
    assert!(index(dir).files.iter().all(|file| !file.unstaged));
}

#[test]
fn discard_keeps_staged_hunks() {
    let dir = tmp();
    let dir = dir.path();
    assert!(init_git_commit(dir, &[("a.txt", "alpha\n")]));
    std::fs::write(dir.join("a.txt"), "beta\n").unwrap();
    stage_file(dir, "a.txt").unwrap();
    std::fs::write(dir.join("a.txt"), "gamma\n").unwrap();
    discard_file(dir, "a.txt").unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.join("a.txt")).unwrap(),
        "beta\n"
    );
    let a = file(dir, "a.txt");
    assert!(a.staged && !a.unstaged);
}

#[test]
fn stage_rejects_path_escape() {
    let dir = tmp();
    for path in ["../secret.txt", "/etc/passwd", "", "a//b"] {
        assert_eq!(stage_file(dir.path(), path).unwrap_err(), "Invalid path");
        assert!(unstage_file(dir.path(), path).is_err());
        assert!(discard_file(dir.path(), path).is_err());
    }
}

#[test]
fn commit_clears_staged_files() {
    let dir = tmp();
    let dir = dir.path();
    assert!(init_git_commit(dir, &[("a.txt", "alpha\n")]));
    std::fs::write(dir.join("a.txt"), "beta\n").unwrap();
    stage_file(dir, "a.txt").unwrap();
    commit(dir, "update a", false).unwrap();
    let index = index(dir);
    assert!(index.files.is_empty());
    assert_eq!(index.head, git_stdout(dir, &["rev-parse", "HEAD"]));
    assert_eq!(head_message(dir).unwrap(), "update a");
}

#[test]
fn commit_rejects_empty_message() {
    let dir = tmp();
    assert_eq!(
        commit(dir.path(), "   ", false).unwrap_err(),
        "Commit message cannot be empty"
    );
}

#[test]
fn commit_amend_rewrites_head_with_staged_changes() {
    let dir = tmp();
    let dir = dir.path();
    assert!(init_git_commit(dir, &[("a.txt", "alpha\n")]));
    std::fs::write(dir.join("a.txt"), "beta\n").unwrap();
    stage_file(dir, "a.txt").unwrap();
    commit(dir, "amended", true).unwrap();
    assert!(index(dir).files.is_empty());
    assert_eq!(
        git_stdout(dir, &["rev-list", "--count", "HEAD"]).as_deref(),
        Some("1")
    );
    assert_eq!(head_message(dir).unwrap(), "amended");
    assert_eq!(
        git_stdout(dir, &["show", "HEAD:a.txt"]).as_deref(),
        Some("beta")
    );
}

#[test]
fn head_message_returns_subject_and_body() {
    let dir = tmp();
    let dir = dir.path();
    assert!(init_git_commit(dir, &[("a.txt", "alpha\n")]));
    std::fs::write(dir.join("a.txt"), "beta\n").unwrap();
    stage_file(dir, "a.txt").unwrap();
    commit(dir, "Subject line\n\nBody text", false).unwrap();
    assert_eq!(head_message(dir).unwrap(), "Subject line\n\nBody text");
}

#[test]
fn head_message_fails_without_commits() {
    let dir = tmp();
    assert!(init_git(dir.path(), "main"));
    assert_eq!(head_message(dir.path()).unwrap_err(), "No commits yet");
}

#[test]
fn signing_hint_ignores_other_errors() {
    let error = "nothing to commit, working tree clean".to_string();
    assert_eq!(with_signing_hint(error.clone()), error);
}

#[test]
fn commit_reports_signing_failure_with_hint() {
    let dir = tmp();
    let dir = dir.path();
    assert!(init_git_commit(dir, &[("a.txt", "a\n")]));
    std::fs::write(dir.join("a.txt"), "b\n").unwrap();
    for args in [
        ["config", "commit.gpgsign", "true"],
        ["config", "gpg.format", "openpgp"],
        ["config", "gpg.program", "/nonexistent/monocode-gpg"],
    ] {
        assert!(git(dir, &args));
    }
    assert!(git(dir, &["add", "."]));
    let error = commit(dir, "signed", false).unwrap_err();
    assert!(error.contains("Git couldn't sign this commit"), "{error}");
}

#[test]
fn staged_context_reads_cached_diff() {
    let dir = tmp();
    let dir = dir.path();
    assert!(init_git_commit(dir, &[("a.txt", "alpha\n")]));
    std::fs::write(dir.join("a.txt"), "beta\n").unwrap();
    stage_file(dir, "a.txt").unwrap();
    let context = staged_context(dir).unwrap();
    assert_eq!(context.branch.as_deref(), Some("main"));
    assert!(context.summary.contains("a.txt"));
    assert!(context.patch.contains("beta"));
}

#[test]
fn staged_context_falls_back_to_unstaged_and_untracked() {
    let dir = tmp();
    let dir = dir.path();
    assert!(init_git_commit(dir, &[("a.txt", "alpha\n")]));
    assert_eq!(staged_context(dir).unwrap_err(), "No changes to summarize");
    std::fs::write(dir.join("a.txt"), "gamma\n").unwrap();
    std::fs::write(dir.join("new.txt"), "n\n").unwrap();
    let context = staged_context(dir).unwrap();
    assert!(context.patch.contains("gamma"));
    assert!(context.summary.ends_with("\nUntracked files:\nnew.txt"));
}

fn with_origin(repo: &Path, origin: &Path) {
    assert!(init_bare(origin));
    let url = origin.to_string_lossy().into_owned();
    assert!(git(repo, &["remote", "add", "origin", &url]));
    assert!(git(repo, &["push", "-u", "origin", "main"]));
}

#[test]
fn sync_counts_unpushed_commits() {
    let (repo, origin) = (tmp(), tmp());
    let repo = repo.path();
    assert!(init_git_commit(repo, &[("a.txt", "alpha\n")]));
    with_origin(repo, origin.path());
    std::fs::write(repo.join("a.txt"), "beta\n").unwrap();
    stage_file(repo, "a.txt").unwrap();
    commit(repo, "second", false).unwrap();
    let index = index(repo);
    assert_eq!(index.remote.as_deref(), Some("origin"));
    assert_eq!(index.upstream.as_deref(), Some("origin/main"));
    assert_eq!(index.default_branch.as_deref(), Some("main"));
    assert_eq!(
        (index.ahead, index.behind, index.ahead_of_default),
        (1, 0, 1)
    );
    assert!(!index.head_pushed);
    push(repo).unwrap();
    let index = super::diff_index(repo).unwrap();
    assert_eq!(index.ahead, 0);
    assert!(index.head_pushed);
}

#[test]
fn feature_branch_without_upstream_counts_and_ranges() {
    let (repo, origin) = (tmp(), tmp());
    let repo = repo.path();
    assert!(init_git_commit(repo, &[("a.txt", "alpha\n")]));
    with_origin(repo, origin.path());
    assert!(git(repo, &["checkout", "-b", "feature"]));
    assert_eq!(
        range_context(repo).unwrap_err(),
        "No commits to include in a pull request"
    );
    std::fs::write(repo.join("a.txt"), "beta\n").unwrap();
    stage_file(repo, "a.txt").unwrap();
    commit(repo, "feature work", false).unwrap();
    let index = index(repo);
    assert_eq!(index.branch.as_deref(), Some("feature"));
    assert_eq!(index.upstream, None);
    assert_eq!((index.ahead, index.ahead_of_default), (1, 1));
    let range = range_context(repo).unwrap();
    assert_eq!(range.base, "main");
    assert_eq!(range.head, "feature");
    assert!(range.commit_summary.contains("feature work"));
    assert!(range.diff_patch.contains("+beta"));
    // First push sets the upstream.
    push(repo).unwrap();
    assert_eq!(
        super::diff_index(repo).unwrap().upstream.as_deref(),
        Some("origin/feature")
    );
}

#[test]
fn push_without_remote_errors() {
    let dir = tmp();
    assert!(init_git_commit(dir.path(), &[("a.txt", "a\n")]));
    assert_eq!(push(dir.path()).unwrap_err(), "No git remote to push to");
}

#[test]
fn sync_pulls_then_pushes() {
    let (origin, a, b) = (tmp(), tmp(), tmp());
    let (a, b): (PathBuf, PathBuf) = (a.path().into(), b.path().into());
    assert!(init_git_commit(&a, &[("a.txt", "alpha\n")]));
    with_origin(&a, origin.path());
    let url = origin.path().to_string_lossy().into_owned();
    assert!(git(
        origin.path(),
        &["symbolic-ref", "HEAD", "refs/heads/main"]
    ));
    assert!(git(
        &b,
        &[
            "clone",
            "-c",
            "core.autocrlf=false",
            "-b",
            "main",
            &url,
            "."
        ]
    ));
    for args in [
        ["config", "user.name", "MonoCode"],
        ["config", "user.email", "monocode@test"],
        ["config", "commit.gpgsign", "false"],
    ] {
        assert!(git(&b, &args));
    }

    std::fs::write(a.join("a.txt"), "beta\n").unwrap();
    stage_file(&a, "a.txt").unwrap();
    commit(&a, "from-a", false).unwrap();
    sync(&a).unwrap();
    pull(&b).unwrap();
    assert_eq!(std::fs::read_to_string(b.join("a.txt")).unwrap(), "beta\n");
    assert_eq!((index(&b).ahead, index(&b).behind), (0, 0));

    std::fs::write(b.join("b.txt"), "from-b\n").unwrap();
    stage_file(&b, "b.txt").unwrap();
    commit(&b, "from-b", false).unwrap();
    std::fs::write(a.join("c.txt"), "from-a-again\n").unwrap();
    stage_file(&a, "c.txt").unwrap();
    commit(&a, "from-a-again", false).unwrap();
    sync(&a).unwrap();
    sync(&b).unwrap();
    assert!(b.join("c.txt").exists());
    sync(&a).unwrap();
    assert!(a.join("b.txt").exists());
}

#[test]
fn parse_gh_pr_list_prefers_open() {
    let json = r#"[{"number":2,"title":"Old","url":"https://example.com/2","state":"MERGED"},{"number":3,"title":"Now","url":"https://example.com/3","state":"OPEN"}]"#;
    let pr = parse_gh_pr_list(json).unwrap();
    assert_eq!(pr.number, 3);
    assert_eq!(pr.state, "open");
    let pr = parse_gh_pr_list(
        r#"[{"number":2,"title":"Old","url":"u","state":"MERGED"},{"number":1,"title":"Older","url":"u","state":"CLOSED"}]"#,
    )
    .unwrap();
    assert_eq!((pr.number, pr.state.as_str()), (2, "merged"));
    assert_eq!(parse_gh_pr_list("[]"), None);
}

#[test]
fn pr_head_filter_qualifies_branch_with_repo_owner() {
    assert_eq!(
        github_pr_head_filter("acme/widget", "feature/x").as_deref(),
        Some("acme:feature/x")
    );
    assert_eq!(github_pr_head_filter("bad", "feature"), None);
}

#[test]
fn split_github_repo_reads_owner_and_name() {
    assert_eq!(
        split_github_repo(" acme/widget ").unwrap(),
        ("acme".into(), "widget".into())
    );
    assert!(split_github_repo("acme").is_err());
    assert!(split_github_repo("acme/a/b").is_err());
    assert!(split_github_repo("ac me/widget").is_err());
}

#[test]
fn pr_create_rejects_empty_title() {
    let dir = tmp();
    assert_eq!(
        pr_create(dir.path(), "  ", "", "main", "feature").unwrap_err(),
        "Pull request title cannot be empty"
    );
}

fn git_cmd_path(cmd: &Command) -> Vec<PathBuf> {
    let path = cmd
        .get_envs()
        .find(|(key, _)| *key == std::ffi::OsStr::new("PATH"))
        .and_then(|(_, value)| value)
        .expect("git should get an explicit PATH");
    std::env::split_paths(path).collect()
}

#[test]
fn read_only_git_cmd_path_adds_fixed_dirs_to_inherited_path() {
    let path = git_cmd_path(&git_cmd());
    assert!(path.contains(&PathBuf::from("/opt/homebrew/bin")));
    assert!(path.contains(&PathBuf::from("/usr/local/bin")));
    if let Some(inherited) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&inherited) {
            assert!(path.contains(&dir), "missing inherited {dir:?}");
        }
    }
}

#[test]
fn local_git_never_resolves_login_shell_path() {
    for action in ["status", "diff", "add", "restore", "cat-file"] {
        let cmd = git_cmd_for_args_with_path(&[action], || {
            panic!("local git must not resolve the login-shell PATH")
        });
        assert!(git_cmd_path(&cmd).contains(&PathBuf::from("/opt/homebrew/bin")));
    }
}

#[test]
fn git_actions_that_need_helpers_use_login_shell_path() {
    for args in [
        &["commit"][..],
        &["push"],
        &["pull"],
        &["fetch"],
        &["clone"],
        &["--literal-pathspecs", "commit"],
    ] {
        let cmd = git_cmd_for_args_with_path(args, || "gui-git-path".into());
        assert!(cmd.get_envs().any(|(key, value)| {
            key == std::ffi::OsStr::new("PATH")
                && value == Some(std::ffi::OsStr::new("gui-git-path"))
        }));
    }
}

#[test]
fn file_diff_reads_each_side_and_flags_binary() {
    let dir = tmp();
    assert!(init_git_commit(
        dir.path(),
        &[("a.txt", "one\ntwo\n"), ("gone.txt", "x\n")]
    ));
    std::fs::write(dir.path().join("a.txt"), "one\nTWO\n").unwrap();
    std::fs::remove_file(dir.path().join("gone.txt")).unwrap();
    std::fs::write(dir.path().join("new.txt"), "fresh\n").unwrap();
    std::fs::write(dir.path().join("bin.dat"), [0u8, 1, 2]).unwrap();
    let unstaged = file_diff(dir.path(), "a.txt", false).unwrap();
    assert_eq!(
        (unstaged.original.as_str(), unstaged.current.as_str()),
        ("one\ntwo\n", "one\nTWO\n")
    );
    assert_eq!(unstaged.status, "modified");
    assert_eq!(
        file_diff(dir.path(), "gone.txt", false).unwrap().status,
        "deleted"
    );
    let fresh = file_diff(dir.path(), "new.txt", false).unwrap();
    assert_eq!(
        (fresh.status.as_str(), fresh.original.as_str()),
        ("untracked", "")
    );
    assert!(file_diff(dir.path(), "bin.dat", false).unwrap().binary);
    assert!(file_diff(dir.path(), "../escape", false).is_err());

    assert!(git(dir.path(), &["add", "a.txt"]));
    std::fs::write(dir.path().join("a.txt"), "one\nTWO\nthree\n").unwrap();
    let staged = file_diff(dir.path(), "a.txt", true).unwrap();
    assert_eq!(
        (staged.original.as_str(), staged.current.as_str()),
        ("one\ntwo\n", "one\nTWO\n")
    );
    assert_eq!(
        file_diff(dir.path(), "a.txt", false).unwrap().original,
        "one\nTWO\n"
    );
}

#[test]
fn stage_contents_stages_a_partial_file_only() {
    let dir = tmp();
    assert!(init_git_commit(dir.path(), &[("a.txt", "one\ntwo\n")]));
    std::fs::write(dir.path().join("a.txt"), "ONE\nTWO\n").unwrap();
    stage_contents(dir.path(), "a.txt", b"ONE\ntwo\n").unwrap();
    assert_eq!(
        file_diff(dir.path(), "a.txt", true).unwrap().current,
        "ONE\ntwo\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "ONE\nTWO\n"
    );
    let file = file(dir.path(), "a.txt");
    assert!(file.staged && file.unstaged);
}

#[test]
fn commit_files_and_diffs_include_the_root_commit() {
    let dir = tmp();
    assert!(init_git_commit(dir.path(), &[("a.txt", "one\n")]));
    let root = git_stdout(dir.path(), &["rev-parse", "HEAD"]).unwrap();
    std::fs::write(dir.path().join("a.txt"), "one\ntwo\n").unwrap();
    std::fs::write(dir.path().join("b.txt"), "bee\n").unwrap();
    assert!(git(dir.path(), &["add", "."]) && git(dir.path(), &["commit", "-m", "second"]));
    let head = git_stdout(dir.path(), &["rev-parse", "HEAD"]).unwrap();
    let files = commit_files(dir.path(), &head).unwrap();
    let summary: Vec<_> = files
        .iter()
        .map(|f| {
            (
                f.relative.as_str(),
                f.status.as_str(),
                f.additions,
                f.deletions,
            )
        })
        .collect();
    assert_eq!(
        summary,
        [("a.txt", "modified", 1, 0), ("b.txt", "added", 1, 0)]
    );
    let diff = commit_file_diff(dir.path(), &head, "a.txt").unwrap();
    assert_eq!(
        (diff.original.as_str(), diff.current.as_str()),
        ("one\n", "one\ntwo\n")
    );
    let first = commit_file_diff(dir.path(), &root, "a.txt").unwrap();
    assert_eq!(
        (first.status.as_str(), first.original.as_str()),
        ("added", "")
    );
    assert_eq!(commit_files(dir.path(), &root).unwrap().len(), 1);
    assert!(commit_files(dir.path(), "not-a-sha").is_err());
}

#[test]
fn an_untracked_files_line_count_is_kept_until_it_changes() {
    let dir = tmp();
    let path = dir.path().join("notes.txt");
    std::fs::write(&path, "one\ntwo\n").unwrap();
    assert_eq!(cached_line_count(&path), 2);
    assert_eq!(cached_line_count(&path), 2);
    std::fs::write(&path, "one\ntwo\nthree\n").unwrap();
    assert_eq!(cached_line_count(&path), 3);
}
