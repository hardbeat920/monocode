//! Descriptive names for worktrees created on a session's first send.
use super::{git, valid_branch, worktrees, Snapshot};
use crate::{
    accounts::Profile,
    agent::Settings,
    provider::{self, Kind, Request},
    scm::text::{limit_section, parse_json_object, string_field},
};
use std::{
    path::Path,
    sync::{atomic::AtomicBool, Arc},
};

/// Recover the original branch from the directory created by `create_worktree`.
/// A selected existing worktree must never be named by a different session.
pub fn temporary_branch(path: &Path, session: i64) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let stamp = name.strip_prefix(&format!("mc-native-{session}-"))?;
    if stamp.is_empty() || !stamp.bytes().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    Some(format!("mc/native-{session}-{stamp}"))
}

fn prompt(message: &str) -> String {
    [
        "You generate concise git branch names.",
        "Return a JSON object with key: branch.",
        "Do not call tools. Reply with JSON only.",
        "Rules:",
        "- Branch should describe the requested work from the user message.",
        "- Keep it short and specific (2-6 words).",
        "- Use plain words only, no issue prefixes and no punctuation-heavy text.",
        "",
        "User message:",
        &limit_section(message, 8_000),
    ]
    .join("\n")
}

/// Tauri's parseBranchName and namedWorktreeBranch, with one `mc/` prefix.
pub fn parse(raw: &str) -> Option<String> {
    let record = parse_json_object(raw);
    let raw = record.as_ref().map_or(raw, |r| string_field(r, "branch"));
    if record.is_none() && raw.contains(['{', '}']) {
        return None;
    }
    let normalized = raw
        .trim()
        .to_ascii_lowercase()
        .replace(['\'', '"', '`'], "");
    let mut fragment = String::new();
    for ch in normalized.chars() {
        let ch = if ch.is_ascii_alphanumeric() || matches!(ch, '/' | '_' | '-') {
            ch
        } else {
            '-'
        };
        if matches!(ch, '/' | '-') && (fragment.is_empty() || fragment.ends_with(ch)) {
            continue;
        }
        fragment.push(ch);
    }
    let fragment = fragment.trim_matches(['/', '_', '-']);
    let fragment = &fragment[..fragment.len().min(64)];
    let fragment = fragment.trim_end_matches(['/', '_', '-']);
    let fragment = fragment
        .strip_prefix("mc/")
        .or_else(|| fragment.strip_prefix("monocode/"))
        .unwrap_or(fragment)
        .trim_matches('/');
    (!fragment.is_empty()).then(|| format!("mc/{fragment}"))
}

/// Uses an isolated text request, preserving the selected provider account.
pub fn generate(
    kind: Kind,
    cwd: &Path,
    message: &str,
    account: Profile,
    cancel: Arc<AtomicBool>,
) -> Option<String> {
    let preferred = kind;
    let (kind, binary) = [
        kind,
        Kind::Claude,
        Kind::Cursor,
        Kind::Codex,
        Kind::Grok,
        Kind::OpenCode,
    ]
    .into_iter()
    .filter(|kind| crate::session_title::supports(*kind))
    .find_map(|kind| provider::resolve(kind).map(|binary| (kind, binary)))?;
    let account = if kind == preferred {
        account
    } else {
        Profile::default()
    };
    let output = provider::run_text(
        Request {
            kind,
            binary,
            cwd: cwd.into(),
            prompt: prompt(message),
            provider_id: None,
            model: match kind {
                Kind::Codex => Some("gpt-5.6-luna".into()),
                Kind::Claude => Some("claude-haiku-4-5".into()),
                _ => None,
            },
            settings: Settings {
                account,
                ..Default::default()
            },
        },
        cancel,
    )
    .ok()?;
    parse(&output)
}

/// A late name may replace only the original temporary branch, never a manual
/// rename, another checkout, or an existing destination branch.
pub fn rename(cwd: &Path, path: &Path, expected: &str, branch: &str) -> Result<Snapshot, String> {
    valid_branch(cwd, branch)?;
    let path = std::fs::canonicalize(path).map_err(|error| error.to_string())?;
    let tree = worktrees(cwd)?
        .into_iter()
        .find(|tree| std::fs::canonicalize(&tree.path).ok().as_ref() == Some(&path))
        .ok_or("This path is not a registered worktree of this repository")?;
    if tree.main || !expected.starts_with("mc/native-") || tree.branch.as_deref() != Some(expected)
    {
        return Err("The worktree no longer uses its temporary branch".into());
    }
    // Naming the source explicitly also protects a rename between the check
    // above and Git's ref transaction. `-m` never overwrites a destination.
    git(cwd, &["branch", "-m", expected, branch])?;
    super::snapshot(&path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_names_are_bounded_and_prefixed_once() {
        for (raw, expected) in [
            (
                "```json\n{\"branch\":\"Fix worktree names\"}\n```",
                Some("mc/fix-worktree-names"),
            ),
            ("`mc/already-named`", Some("mc/already-named")),
            ("monocode/faster-worktrees", Some("mc/faster-worktrees")),
            (
                " /Feature///Better   names/ ",
                Some("mc/feature/better-names"),
            ),
            ("{broken json}", None),
            ("{\"branch\":null}", None),
            ("...", None),
            ("", None),
        ] {
            assert_eq!(parse(raw).as_deref(), expected, "{raw}");
        }
        assert_eq!(parse(&"a".repeat(100)).unwrap().len(), 67);
        assert!(prompt(&"🙂".repeat(8_000)).contains("[truncated]"));
        assert_eq!(
            temporary_branch(Path::new("/repo/mc-native-42-123"), 42).as_deref(),
            Some("mc/native-42-123")
        );
        assert!(temporary_branch(Path::new("/repo/mc-native-42-123"), 4).is_none());
        assert!(temporary_branch(Path::new("/repo/feature"), 42).is_none());
    }

    #[test]
    fn naming_preserves_manual_branches_collisions_and_worktree_contents() {
        let temp = tempfile::tempdir().unwrap();
        let cwd = temp.path().join("project");
        std::fs::create_dir(&cwd).unwrap();
        git(&cwd, &["init", "-b", "main"]).unwrap();
        git(
            &cwd,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--allow-empty",
                "-m",
                "Base",
            ],
        )
        .unwrap();
        let path = super::super::create_worktree(&cwd, "HEAD", 42).unwrap();
        let expected = temporary_branch(&path, 42).unwrap();
        std::fs::write(path.join("keep.txt"), "unsaved work").unwrap();
        git(&cwd, &["branch", "mc/existing"]).unwrap();
        assert!(rename(&cwd, &path, &expected, "mc/existing").is_err());
        assert_eq!(
            super::super::snapshot(&path).unwrap().current.as_deref(),
            Some(expected.as_str())
        );
        let named = rename(&cwd, &path, &expected, "mc/fix-worktree-names").unwrap();
        assert_eq!(named.current.as_deref(), Some("mc/fix-worktree-names"));
        assert_eq!(
            std::fs::read_to_string(path.join("keep.txt")).unwrap(),
            "unsaved work"
        );
        assert_eq!(
            super::super::snapshot(&cwd).unwrap().current.as_deref(),
            Some("main")
        );
        assert!(rename(&cwd, &path, &expected, "mc/late-result").is_err());

        let manual = super::super::create_worktree(&cwd, "HEAD", 43).unwrap();
        let expected = temporary_branch(&manual, 43).unwrap();
        git(&manual, &["branch", "-m", "mc/my-manual-name"]).unwrap();
        assert!(rename(&cwd, &manual, &expected, "mc/late-result").is_err());
        assert_eq!(
            super::super::snapshot(&manual).unwrap().current.as_deref(),
            Some("mc/my-manual-name")
        );
        assert!(rename(&cwd, &cwd, "main", "mc/rename-main").is_err());
        assert!(rename(&cwd, &manual, "mc/my-manual-name", "-invalid").is_err());
    }
}
