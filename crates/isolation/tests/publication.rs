#![cfg(target_os = "macos")]
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

struct Fixture {
    root: PathBuf,
    source: PathBuf,
    copy: PathBuf,
    remote: PathBuf,
    base: String,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn command(root: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .current_dir(root)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap()
}
fn git(root: &Path, args: &[&str]) -> String {
    let output = command(root, args);
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
impl Fixture {
    fn new(label: &str) -> Option<Self> {
        monocode_isolation::set_publication_helper(PathBuf::from(env!(
            "CARGO_BIN_EXE_monocode-isolation"
        )));
        let root =
            std::env::temp_dir().join(format!("monocode-publication-{}", uuid::Uuid::new_v4()));
        let source = root.join("source");
        fs::create_dir_all(&source).unwrap();
        git(&source, &["init", "-b", "main"]);
        git(&source, &["config", "user.name", "Fixture"]);
        git(
            &source,
            &["config", "user.email", "fixture@example.invalid"],
        );
        git(&source, &["config", "commit.gpgsign", "false"]);
        let lines = (0..40).map(|n| format!("line {n}\n")).collect::<String>();
        fs::write(source.join("shared"), lines).unwrap();
        fs::write(source.join("inherited"), "original\n").unwrap();
        fs::write(source.join("deleted-before-copy"), "keep in public HEAD\n").unwrap();
        git(&source, &["add", "-A"]);
        git(&source, &["commit", "-m", "Base"]);
        let base = git(&source, &["rev-parse", "HEAD"]);
        let remote = root.join("remote.git");
        git(
            &source,
            &[
                "clone",
                "--bare",
                source.to_str().unwrap(),
                remote.to_str().unwrap(),
            ],
        );
        git(
            &source,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        fs::write(source.join("inherited"), "inherited secret edit\n").unwrap();
        fs::write(
            source.join("untracked-before-copy"),
            "inherited untracked\n",
        )
        .unwrap();
        fs::remove_file(source.join("deleted-before-copy")).unwrap();
        let shared = fs::read_to_string(source.join("shared"))
            .unwrap()
            .replace("line 3\n", "inherited line 3\n");
        fs::write(source.join("shared"), shared).unwrap();
        let registry = root.join("registry");
        let supported = monocode_isolation::dispatch(
            &registry,
            json!({"command":"cow_capability","args":{"cwd":source}}),
        )
        .unwrap();
        if supported["supported"] != true {
            assert!(
                std::env::var_os("MONOCODE_REQUIRE_COW").is_none(),
                "APFS required"
            );
            fs::remove_dir_all(root).unwrap();
            return None;
        }
        let copy = monocode_isolation::dispatch(
            &registry,
            json!({"command":"cow_create","args":{"cwd":source,"sessionId":label}}),
        )
        .unwrap();
        Some(Self {
            root,
            source,
            copy: PathBuf::from(copy["path"].as_str().unwrap()),
            remote,
            base,
        })
    }
    fn action(&self, action: &str, args: Value) -> Result<Value, String> {
        monocode_isolation::publication(&self.copy, action, args).map(|value| value.unwrap())
    }
    fn add_new_work(&self) {
        let shared = fs::read_to_string(self.copy.join("shared"))
            .unwrap()
            .replace("line 30\n", "new line 30\n");
        fs::write(self.copy.join("shared"), shared).unwrap();
        fs::write(self.copy.join("new"), "new file\n").unwrap();
    }
    fn assert_public(&self) {
        assert_eq!(git(&self.copy, &["show", "HEAD:inherited"]), "original");
        assert!(!command(
            &self.copy,
            &["cat-file", "-e", "HEAD:untracked-before-copy"]
        )
        .status
        .success());
        assert_eq!(
            git(&self.copy, &["show", "HEAD:deleted-before-copy"]),
            "keep in public HEAD"
        );
        let shared = git(&self.copy, &["show", "HEAD:shared"]);
        assert!(shared.contains("new line 30"));
        assert!(!shared.contains("inherited line 3"));
        assert!(fs::read_to_string(self.copy.join("shared"))
            .unwrap()
            .contains("inherited line 3"));
        assert_eq!(
            fs::read_to_string(self.source.join("inherited")).unwrap(),
            "inherited secret edit\n"
        );
    }
}

#[test]
fn normal_git_commit_amend_and_push_publish_only_new_edits() {
    let Some(f) = Fixture::new("normal-git") else {
        return;
    };
    let initial = f.action("trees", json!({})).unwrap();
    assert_eq!(initial["head"], initial["index"]);
    assert_eq!(initial["index"], initial["worktree"]);
    f.add_new_work();
    git(&f.copy, &["add", "-A"]);
    git(&f.copy, &["commit", "-m", "Only new work"]);
    f.assert_public();
    let settled = f.action("trees", json!({})).unwrap();
    assert_eq!(settled["head"], settled["worktree"]);
    fs::write(f.copy.join("new"), "new file\namend\n").unwrap();
    git(&f.copy, &["add", "-A"]);
    git(&f.copy, &["commit", "--amend", "--no-edit"]);
    f.assert_public();
    let branch = git(&f.copy, &["branch", "--show-current"]);
    git(&f.copy, &["push", "-u", "origin", "HEAD"]);
    assert_eq!(
        git(&f.remote, &["show", &format!("{branch}:inherited")]),
        "original"
    );
    assert_eq!(
        git(&f.remote, &["show", &format!("{branch}:new")]),
        "new file\namend"
    );
}

#[test]
fn conflicting_edits_fail_without_changing_index_head_worktree_or_remote() {
    let Some(f) = Fixture::new("conflict") else {
        return;
    };
    fs::write(f.copy.join("inherited"), "new overlapping edit\n").unwrap();
    let index = fs::read(f.copy.join(".git/index")).unwrap();
    assert!(f.action("stage", json!({"mode":"all"})).is_err());
    assert_eq!(fs::read(f.copy.join(".git/index")).unwrap(), index);
    assert_eq!(git(&f.copy, &["rev-parse", "HEAD"]), f.base);
    git(&f.copy, &["add", "-A"]);
    assert!(!command(&f.copy, &["commit", "-m", "Must fail"])
        .status
        .success());
    assert_eq!(
        fs::read_to_string(f.copy.join("inherited")).unwrap(),
        "new overlapping edit\n"
    );
    git(
        &f.copy,
        &[
            "-c",
            "core.hooksPath=/dev/null",
            "commit",
            "-m",
            "Legacy unsafe commit",
        ],
    );
    assert!(!command(&f.copy, &["push", "origin", "HEAD"])
        .status
        .success());
    let branch = git(&f.copy, &["branch", "--show-current"]);
    assert!(!command(
        &f.remote,
        &["show-ref", "--verify", &format!("refs/heads/{branch}")]
    )
    .status
    .success());
}

#[test]
fn legacy_history_is_repaired_with_a_private_backup_before_publication() {
    let Some(f) = Fixture::new("legacy") else {
        return;
    };
    f.add_new_work();
    git(&f.copy, &["add", "-A"]);
    git(
        &f.copy,
        &[
            "-c",
            "core.hooksPath=/dev/null",
            "commit",
            "-m",
            "Legacy work",
        ],
    );
    let raw = git(&f.copy, &["rev-parse", "HEAD"]);
    assert!(!command(&f.copy, &["push", "origin", "HEAD"])
        .status
        .success());
    f.action("canonicalize", json!({})).unwrap();
    f.action("prepare", json!({})).unwrap();
    assert_ne!(git(&f.copy, &["rev-parse", "HEAD"]), raw);
    assert!(git(
        &f.copy,
        &[
            "for-each-ref",
            "--format=%(objectname)",
            "refs/monocode/private"
        ]
    )
    .contains(&raw));
    f.assert_public();
    git(&f.copy, &["push", "origin", "HEAD"]);
}

#[test]
fn configured_clean_filters_and_crlf_keep_the_same_publication_boundary() {
    let Some(f) = Fixture::new("conversion") else {
        return;
    };
    git(
        &f.copy,
        &[
            "config",
            "filter.fixture.clean",
            "sed 's/inherited/encoded/g'",
        ],
    );
    git(
        &f.copy,
        &[
            "config",
            "filter.fixture.smudge",
            "sed 's/encoded/inherited/g'",
        ],
    );
    git(&f.copy, &["config", "filter.fixture.required", "true"]);
    git(&f.copy, &["config", "core.autocrlf", "true"]);
    fs::write(f.copy.join(".git/info/attributes"), "* filter=fixture\n").unwrap();
    f.add_new_work();
    let raw = fs::read_to_string(f.copy.join("shared"))
        .unwrap()
        .replace('\n', "\r\n");
    fs::write(f.copy.join("shared"), &raw).unwrap();
    f.action("stage", json!({"mode":"all"})).unwrap();
    git(&f.copy, &["commit", "-m", "Filtered new work"]);
    f.assert_public();
    assert_eq!(fs::read_to_string(f.copy.join("shared")).unwrap(), raw);
    let settled = f.action("trees", json!({})).unwrap();
    assert_eq!(settled["head"], settled["worktree"]);
    git(&f.copy, &["push", "origin", "HEAD"]);
}

#[test]
fn binary_overlap_is_not_published() {
    let Some(f) = Fixture::new("binary") else {
        return;
    };
    fs::write(f.copy.join("inherited"), b"new binary\0private").unwrap();
    let index = fs::read(f.copy.join(".git/index")).unwrap();
    assert!(f.action("stage", json!({"mode":"all"})).is_err());
    assert_eq!(fs::read(f.copy.join(".git/index")).unwrap(), index);
    assert_eq!(git(&f.copy, &["rev-parse", "HEAD"]), f.base);
}

#[test]
fn initially_ignored_files_cannot_be_added_by_changing_ignore_rules() {
    let Some(f) = Fixture::new("ignored") else {
        return;
    };
    let source = f.root.join("ignored-source");
    fs::create_dir(&source).unwrap();
    git(&source, &["init", "-b", "main"]);
    git(&source, &["config", "user.name", "Fixture"]);
    git(
        &source,
        &["config", "user.email", "fixture@example.invalid"],
    );
    git(&source, &["config", "commit.gpgsign", "false"]);
    fs::write(source.join(".gitignore"), "private.env\n").unwrap();
    git(&source, &["add", "-A"]);
    git(&source, &["commit", "-m", "Ignore private input"]);
    fs::write(source.join("private.env"), "inherited secret\n").unwrap();
    let copy = monocode_isolation::dispatch(
        &f.root.join("registry"),
        json!({"command":"cow_create","args":{"cwd":source,"sessionId":"ignored-source"}}),
    )
    .unwrap();
    let copy = PathBuf::from(copy["path"].as_str().unwrap());
    fs::write(copy.join(".gitignore"), "").unwrap();
    assert!(
        monocode_isolation::publication(&copy, "stage", json!({"mode":"all"}))
            .unwrap_err()
            .contains("initially ignored")
    );
    assert!(git(&copy, &["diff", "--cached"]).is_empty());
}

#[test]
fn signed_legacy_history_is_never_silently_resigned_or_stripped() {
    for header in ["gpgsig", "gpgsig-sha256"] {
        let Some(f) = Fixture::new("signed") else {
            return;
        };
        f.add_new_work();
        git(&f.copy, &["add", "-A"]);
        git(
            &f.copy,
            &[
                "-c",
                "core.hooksPath=/dev/null",
                "commit",
                "-m",
                "Private legacy commit",
            ],
        );
        let raw = git(&f.copy, &["cat-file", "commit", "HEAD"]);
        let (headers, message) = raw.split_once("\n\n").unwrap();
        let signed = format!("{headers}\n{header} -----BEGIN PGP SIGNATURE-----\n fixture-signature\n -----END PGP SIGNATURE-----\n\n{message}\n");
        use std::io::Write;
        use std::process::Stdio;
        let mut child = Command::new("git")
            .current_dir(&f.copy)
            .args(["hash-object", "-w", "-t", "commit", "--stdin"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(signed.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        let oid = String::from_utf8(output.stdout).unwrap();
        git(&f.copy, &["update-ref", "HEAD", oid.trim()]);
        assert!(f
            .action("canonicalize", json!({}))
            .unwrap_err()
            .contains("signed"));
        assert_eq!(git(&f.copy, &["rev-parse", "HEAD"]), oid.trim());
    }
}

#[test]
fn cleanup_refuses_staged_edits_even_when_working_files_match_the_baseline() {
    let Some(f) = Fixture::new("staged-cleanup") else {
        return;
    };
    f.add_new_work();
    f.action("stage", json!({"mode":"all"})).unwrap();
    fs::write(
        f.copy.join("shared"),
        fs::read(f.source.join("shared")).unwrap(),
    )
    .unwrap();
    fs::remove_file(f.copy.join("new")).unwrap();
    let index = fs::read(f.copy.join(".git/index")).unwrap();
    let owner = monocode_isolation::dispatch(
        &f.root.join("registry"),
        json!({"command":"cow_owner","args":{"cwd":f.source,"path":f.copy}}),
    )
    .unwrap();
    assert!(monocode_isolation::dispatch(&f.root.join("registry"), json!({"command":"cow_check_remove","args":{"cwd":f.source,"cowId":owner["id"],"force":false}})).is_err());
    assert_eq!(fs::read(f.copy.join(".git/index")).unwrap(), index);
    assert_eq!(git(&f.copy, &["show", ":new"]), "new file");
    assert!(f.copy.exists());
}

#[test]
fn fetched_upstream_commits_are_not_removed_when_they_match_inherited_edits() {
    let Some(f) = Fixture::new("upstream") else {
        return;
    };
    git(&f.source, &["add", "-A"]);
    git(&f.source, &["commit", "-m", "Legitimate upstream work"]);
    git(&f.source, &["push", "origin", "main"]);
    let upstream = git(&f.source, &["rev-parse", "HEAD"]);
    git(&f.copy, &["fetch", "origin"]);
    git(&f.copy, &["reset", "--hard", "origin/main"]);
    f.action("canonicalize", json!({})).unwrap();
    assert_eq!(git(&f.copy, &["rev-parse", "HEAD"]), upstream);
    assert_eq!(
        git(&f.copy, &["show", "HEAD:inherited"]),
        "inherited secret edit"
    );
    fs::write(
        f.copy.join("shared"),
        fs::read_to_string(f.copy.join("shared"))
            .unwrap()
            .replace("inherited line 3\n", "new post-upstream line 3\n"),
    )
    .unwrap();
    git(&f.copy, &["commit", "-am", "New work after upstream"]);
    assert_eq!(git(&f.copy, &["rev-parse", "HEAD^"]), upstream);
    assert!(git(&f.copy, &["show", "HEAD:shared"]).contains("new post-upstream line 3"));
    git(&f.copy, &["push", "origin", "HEAD"]);
}

#[test]
fn discard_after_upstream_advance_keeps_upstream_and_inherited_contents() {
    let Some(f) = Fixture::new("upstream-discard") else {
        return;
    };
    let original = git(&f.source, &["show", "HEAD:shared"]) + "\n";
    fs::write(
        f.source.join("shared"),
        original.replace("line 20\n", "upstream line 20\n"),
    )
    .unwrap();
    git(&f.source, &["add", "shared"]);
    git(&f.source, &["commit", "-m", "Upstream work"]);
    git(&f.source, &["push", "origin", "main"]);
    git(&f.copy, &["fetch", "origin"]);
    git(&f.copy, &["reset", "--hard", "origin/main"]);
    let inherited = fs::read_to_string(f.copy.join("shared"))
        .unwrap()
        .replace("line 3\n", "inherited line 3\n");
    fs::write(
        f.copy.join("shared"),
        inherited.replace("line 30\n", "session line 30\n"),
    )
    .unwrap();
    f.action("discard", json!({"relative":"shared"})).unwrap();
    assert_eq!(
        fs::read_to_string(f.copy.join("shared")).unwrap(),
        inherited
    );
    let trees = f.action("trees", json!({})).unwrap();
    assert_eq!(trees["head"], trees["worktree"]);
}

#[test]
fn reverting_our_own_published_edit_remains_a_new_change() {
    let Some(f) = Fixture::new("published-revert") else {
        return;
    };
    fs::write(f.copy.join("new"), "published work\n").unwrap();
    f.action("stage", json!({"mode":"all"})).unwrap();
    git(&f.copy, &["commit", "-m", "New work"]);
    git(&f.copy, &["push", "-u", "origin", "HEAD"]);
    fs::remove_file(f.copy.join("new")).unwrap();
    let trees = f.action("trees", json!({})).unwrap();
    assert_ne!(trees["head"], trees["worktree"]);
    f.action("stage", json!({"mode":"all"})).unwrap();
    git(&f.copy, &["commit", "-m", "Revert new work"]);
    assert!(!command(&f.copy, &["cat-file", "-e", "HEAD:new"])
        .status
        .success());
    assert_eq!(git(&f.copy, &["show", "HEAD:inherited"]), "original");
    git(&f.copy, &["push", "origin", "HEAD"]);
}

#[test]
fn legacy_commits_before_an_upstream_merge_keep_their_own_publication_base() {
    let Some(f) = Fixture::new("legacy-upstream-merge") else {
        return;
    };
    f.add_new_work();
    git(&f.copy, &["add", "-A"]);
    git(
        &f.copy,
        &[
            "-c",
            "core.hooksPath=/dev/null",
            "commit",
            "-m",
            "Legacy work",
        ],
    );
    let original = git(&f.source, &["show", "HEAD:shared"]) + "\n";
    fs::write(
        f.source.join("shared"),
        original.replace("line 20\n", "upstream line 20\n"),
    )
    .unwrap();
    git(&f.source, &["add", "shared"]);
    git(&f.source, &["commit", "-m", "Upstream work"]);
    git(&f.source, &["push", "origin", "main"]);
    git(&f.copy, &["fetch", "origin"]);
    git(
        &f.copy,
        &[
            "-c",
            "core.hooksPath=/dev/null",
            "merge",
            "--no-edit",
            "origin/main",
        ],
    );
    f.action("canonicalize", json!({})).unwrap();
    let parent = git(&f.copy, &["show", "HEAD^:shared"]);
    assert!(parent.contains("new line 30"));
    assert!(!parent.contains("inherited line 3"));
    assert!(!parent.contains("upstream line 20"));
    let head = git(&f.copy, &["show", "HEAD:shared"]);
    assert!(head.contains("new line 30"));
    assert!(head.contains("upstream line 20"));
    assert!(!head.contains("inherited line 3"));
    git(&f.copy, &["push", "origin", "HEAD"]);
}

#[test]
fn prepare_refuses_to_overwrite_concurrent_staging() {
    use std::{
        io::Write,
        os::unix::fs::PermissionsExt,
        time::{Duration, Instant},
    };
    let Some(f) = Fixture::new("index-race") else {
        return;
    };
    f.add_new_work();
    git(&f.copy, &["add", "-A"]);
    let ready = f.root.join("filter-ready");
    let resume = f.root.join("filter-resume");
    let script = f.root.join("slow-filter");
    let mut file = fs::File::create(&script).unwrap();
    write!(file, "#!/bin/sh\nif [ ! -f '{}' ]; then\n touch '{}'\n while [ ! -f '{}' ]; do sleep 0.01; done\nfi\ncat\n", ready.display(), ready.display(), resume.display()).unwrap();
    drop(file);
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    git(
        &f.copy,
        &["config", "filter.slow.clean", script.to_str().unwrap()],
    );
    git(&f.copy, &["config", "filter.slow.required", "true"]);
    fs::write(f.copy.join(".git/info/attributes"), "* filter=slow\n").unwrap();
    let copy = f.copy.clone();
    let worker =
        std::thread::spawn(move || monocode_isolation::publication(&copy, "prepare", json!({})));
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let started = ready.exists();
    fs::write(f.copy.join("concurrent"), "concurrent staged work\n").unwrap();
    let staged = command(&f.copy, &["add", "concurrent"]);
    fs::write(&resume, "continue").unwrap();
    let result = worker.join().unwrap();
    assert!(started && staged.status.success());
    assert!(result.unwrap_err().contains("changed"));
    assert_eq!(
        git(&f.copy, &["show", ":concurrent"]),
        "concurrent staged work"
    );
    assert_eq!(git(&f.copy, &["rev-parse", "HEAD"]), f.base);
}
