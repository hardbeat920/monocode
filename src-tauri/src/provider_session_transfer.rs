//! Carry a Claude or Codex conversation to another account of the same provider.
//! Each account keeps its native transcripts under its own home, so a usage
//! limit on one account would otherwise strand the conversation there. Both
//! CLIs resume from the transcript on disk, so copying the exact files keeps
//! the provider context (thinking, tool history, subagents) byte for byte.

use std::path::{Path, PathBuf};

use tauri::AppHandle;

use crate::codex_mono_store::{
    apply_agent_edges, closed_agent_edges, copy_durable, find_thread, history_base, rollout_dest,
    rollout_matches, state_db, validate_thread_id,
};

#[tauri::command(async)]
pub fn provider_session_transfer(
    app: AppHandle,
    provider: String,
    from_account_id: Option<String>,
    to_account_id: Option<String>,
    provider_session_id: String,
) -> Result<(), String> {
    validate_thread_id(&provider_session_id)
        .map_err(|_| "Invalid provider session id".to_string())?;
    let from = provider_home(&app, &provider, from_account_id.as_deref())?;
    let to = provider_home(&app, &provider, to_account_id.as_deref())?;
    if same_path(&from, &to) {
        return Ok(());
    }
    match provider.as_str() {
        "claude" => transfer_claude(&from, &to, &provider_session_id),
        "codex" => transfer_codex(&from, &to, &provider_session_id),
        _ => Err("Provider accounts are not supported for this provider".into()),
    }
}

fn provider_home(
    app: &AppHandle,
    provider: &str,
    account_id: Option<&str>,
) -> Result<PathBuf, String> {
    if let Some(dir) = crate::harness::provider_account_dir(app, provider, account_id)? {
        return Ok(dir);
    }
    let (var, name) = match provider {
        "claude" => ("CLAUDE_CONFIG_DIR", ".claude"),
        "codex" => ("CODEX_HOME", ".codex"),
        _ => return Err("Provider accounts are not supported for this provider".into()),
    };
    std::env::var_os(var)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| crate::dirs_home().map(|home| PathBuf::from(home).join(name)))
        .ok_or_else(|| format!("Could not find the {provider} home directory"))
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// Claude keeps `projects/<cwd>/<id>.jsonl`, with subagent transcripts and
/// large tool results in `projects/<cwd>/<id>/` and edit checkpoints in
/// `file-history/<id>/`. The transcript is copied last: it is what `--resume`
/// looks for, so a partial copy is never mistaken for a complete one.
fn transfer_claude(from: &Path, to: &Path, id: &str) -> Result<(), String> {
    let name = format!("{id}.jsonl");
    let project = std::fs::read_dir(from.join("projects"))
        .ok()
        .and_then(|projects| {
            projects
                .flatten()
                .map(|entry| entry.path())
                .find(|dir| dir.join(&name).is_file())
        })
        .ok_or("Could not find the Claude conversation to carry over")?;
    let dest_project = to.join("projects").join(
        project
            .file_name()
            .ok_or("Invalid Claude project directory")?,
    );
    std::fs::create_dir_all(&dest_project).map_err(|e| e.to_string())?;
    copy_tree(&project.join(id), &dest_project.join(id))?;
    copy_tree(
        &from.join("file-history").join(id),
        &to.join("file-history").join(id),
    )?;
    copy_durable(&project.join(&name), &dest_project.join(&name))
        .map_err(|e| format!("Could not carry the Claude conversation over: {e}"))
}

fn copy_tree(source: &Path, dest: &Path) -> Result<(), String> {
    let entries = match std::fs::read_dir(source) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.to_string()),
    };
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        let target = dest.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            copy_durable(&entry.path(), &target).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Codex resolves a thread id to its rollout file. Spawned agents and forked
/// history live in their own rollouts, so those are carried along, and the
/// root is copied last for the same reason as Claude's transcript.
fn transfer_codex(from: &Path, to: &Path, id: &str) -> Result<(), String> {
    let roots = [from.join("sessions"), from.join("archived_sessions")]
        .into_iter()
        .filter(|root| root.is_dir())
        .collect::<Vec<_>>();
    let find = |thread: &str| -> Result<Option<PathBuf>, String> {
        for root in &roots {
            if let Some(path) = find_thread(root, thread)? {
                return Ok(Some(path));
            }
        }
        Ok(None)
    };
    let root = find(id)?.ok_or("Could not find the Codex conversation to carry over")?;
    let sqlite_home = std::env::var_os("CODEX_SQLITE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| from.to_path_buf());

    let mut files = vec![root];
    let mut index = 0;
    let mut pending: Vec<String> = descendant_threads(&sqlite_home, id)?;
    loop {
        while index < files.len() {
            // Compressed rollouts can retain a plain JSONL append tail.
            let path = files[index].clone();
            let sibling = if path.extension().is_some_and(|ext| ext == "zst") {
                path.with_extension("")
            } else {
                let mut compressed = path.as_os_str().to_os_string();
                compressed.push(".zst");
                PathBuf::from(compressed)
            };
            if sibling.is_file() && !files.contains(&sibling) {
                files.push(sibling);
            }
            if let Some(base) = history_base(&path)? {
                validate_thread_id(&base)?;
                pending.push(base);
            }
            index += 1;
        }
        let Some(thread) = pending.pop() else { break };
        if files.iter().any(|path| rollout_matches(path, &thread)) {
            continue;
        }
        // A spawned agent that never wrote a rollout has nothing to resume.
        if let Some(path) = find(&thread)? {
            files.push(path);
        }
    }

    for path in files.iter().rev() {
        let dest = rollout_dest(to, path)?;
        copy_durable(path, &dest)
            .map_err(|e| format!("Could not carry the Codex conversation over: {e}"))?;
    }
    // Closed agents are recorded only in SQLite. Without them, resuming would
    // treat finished subagents as still open.
    let edges = closed_agent_edges(&sqlite_home, id)?;
    if !edges.is_empty() {
        if let Some(db) = state_db(to)? {
            apply_agent_edges(&db, edges)?;
        }
    }
    Ok(())
}

fn descendant_threads(sqlite_home: &Path, id: &str) -> Result<Vec<String>, String> {
    let Some(path) = state_db(sqlite_home)? else {
        return Ok(Vec::new());
    };
    let db =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| e.to_string())?;
    db.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| e.to_string())?;
    let exists: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='thread_spawn_edges')",
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if !exists {
        return Ok(Vec::new());
    }
    let mut statement = db
        .prepare("WITH RECURSIVE subtree(child) AS (SELECT child_thread_id FROM thread_spawn_edges WHERE parent_thread_id = ?1 UNION SELECT edge.child_thread_id FROM thread_spawn_edges edge JOIN subtree ON edge.parent_thread_id = subtree.child) SELECT child FROM subtree")
        .map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([id], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    let mut threads = Vec::new();
    for row in rows {
        let thread = row.map_err(|e| e.to_string())?;
        if validate_thread_id(&thread).is_ok() {
            threads.push(thread);
        }
    }
    Ok(threads)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!(
                "monocode-session-transfer-{}",
                uuid::Uuid::new_v4()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        fn from(&self) -> PathBuf {
            self.0.join("from")
        }
        fn to(&self) -> PathBuf {
            self.0.join("to")
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn carries_a_claude_transcript_with_its_subagents_and_checkpoints() {
        let fixture = Fixture::new();
        let project = fixture.from().join("projects").join("-Users-me-app");
        std::fs::create_dir_all(project.join("abc").join("subagents")).unwrap();
        std::fs::write(project.join("abc.jsonl"), "{\"turn\":2}\n").unwrap();
        std::fs::write(
            project.join("abc").join("subagents").join("agent-1.jsonl"),
            "x",
        )
        .unwrap();
        std::fs::create_dir_all(fixture.from().join("file-history").join("abc")).unwrap();
        std::fs::write(
            fixture.from().join("file-history").join("abc").join("v1"),
            "y",
        )
        .unwrap();
        std::fs::write(project.join("other.jsonl"), "unrelated").unwrap();
        // An older copy from an earlier switch is replaced by the newer one.
        let dest = fixture.to().join("projects").join("-Users-me-app");
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::write(dest.join("abc.jsonl"), "{\"turn\":1}\n").unwrap();

        transfer_claude(&fixture.from(), &fixture.to(), "abc").unwrap();

        assert_eq!(
            std::fs::read_to_string(dest.join("abc.jsonl")).unwrap(),
            "{\"turn\":2}\n"
        );
        assert!(dest
            .join("abc")
            .join("subagents")
            .join("agent-1.jsonl")
            .is_file());
        assert!(fixture
            .to()
            .join("file-history")
            .join("abc")
            .join("v1")
            .is_file());
        assert!(!dest.join("other.jsonl").exists());
    }

    #[test]
    fn reports_a_missing_claude_transcript() {
        let fixture = Fixture::new();
        std::fs::create_dir_all(fixture.from().join("projects")).unwrap();
        assert!(transfer_claude(&fixture.from(), &fixture.to(), "missing").is_err());
    }

    #[test]
    fn carries_a_codex_rollout_and_its_inherited_history() {
        let fixture = Fixture::new();
        let write = |dir: &str, id: &str, base: Option<&str>| {
            let dir = fixture.from().join(dir).join("2026").join("10").join("08");
            std::fs::create_dir_all(&dir).unwrap();
            let meta = serde_json::json!({"type":"session_meta","payload":{"id":id,"history_base":base.map(|id| serde_json::json!({"thread_id":id}))}});
            std::fs::write(
                dir.join(format!("rollout-2026-10-08T10-00-00-{id}.jsonl")),
                format!("{meta}\n"),
            )
            .unwrap();
        };
        write("sessions", "root-1", Some("base-1"));
        write("archived_sessions", "base-1", None);
        write("sessions", "unrelated-1", None);

        transfer_codex(&fixture.from(), &fixture.to(), "root-1").unwrap();

        let day = fixture
            .to()
            .join("sessions")
            .join("2026")
            .join("10")
            .join("08");
        assert!(day
            .join("rollout-2026-10-08T10-00-00-root-1.jsonl")
            .is_file());
        assert!(day
            .join("rollout-2026-10-08T10-00-00-base-1.jsonl")
            .is_file());
        assert!(!day
            .join("rollout-2026-10-08T10-00-00-unrelated-1.jsonl")
            .exists());
    }
}
