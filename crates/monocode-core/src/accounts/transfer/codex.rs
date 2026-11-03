use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
#[derive(Serialize, Deserialize)]
pub(crate) struct AgentEdge {
    parent: String,
    child: String,
    status: String,
}
pub(crate) fn validate_thread_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err("Invalid Codex thread id".into());
    }
    Ok(())
}

pub(crate) fn rollout_dest(home: &Path, path: &Path) -> Result<PathBuf, String> {
    let name = path
        .file_name()
        .ok_or("Missing Codex rollout filename")?
        .to_string_lossy();
    if !path.is_file()
        || !name.starts_with("rollout-")
        || !(name.ends_with(".jsonl") || name.ends_with(".jsonl.zst"))
    {
        return Err("Not a saved Codex rollout file".into());
    }
    let date = name
        .get(8..18)
        .filter(|date| {
            date.bytes().enumerate().all(|(i, b)| {
                if i == 4 || i == 7 {
                    b == b'-'
                } else {
                    b.is_ascii_digit()
                }
            })
        })
        .ok_or("Missing Codex rollout date")?;
    let dir = home
        .join("sessions")
        .join(&date[..4])
        .join(&date[5..7])
        .join(&date[8..10]);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join(path.file_name().unwrap()))
}

pub(crate) fn copy_durable(path: &Path, dest: &Path) -> std::io::Result<()> {
    let mut temp = dest.as_os_str().to_os_string();
    temp.push(format!(".{}.tmp", rand::random::<u128>()));
    let temp = PathBuf::from(temp);
    let result = std::fs::copy(path, &temp)
        .and_then(|_| {
            // Windows requires write access when flushing a file to disk.
            std::fs::File::options()
                .write(true)
                .open(&temp)
                .and_then(|f| f.sync_all())
        })
        .and_then(|()| std::fs::rename(&temp, dest));
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

pub(crate) fn state_db(home: &Path) -> Result<Option<PathBuf>, String> {
    let entries = match std::fs::read_dir(home) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let mut databases = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(version) = name
            .strip_prefix("state_")
            .and_then(|n| n.strip_suffix(".sqlite"))
            .and_then(|n| n.parse::<u32>().ok())
        {
            if entry.file_type().map_err(|e| e.to_string())?.is_file() {
                databases.push((version, entry.path()));
            }
        }
    }
    Ok(databases
        .into_iter()
        .max_by_key(|(version, _)| *version)
        .map(|(_, path)| path))
}

pub(crate) fn closed_agent_edges(home: &Path, thread_id: &str) -> Result<Vec<AgentEdge>, String> {
    validate_thread_id(thread_id)?;
    let Some(path) = state_db(home)? else {
        return Ok(Vec::new());
    };
    let db =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| e.to_string())?;
    db.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| e.to_string())?;
    let exists: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='thread_spawn_edges')", [], |row| row.get(0)).map_err(|e| e.to_string())?;
    if !exists {
        return Ok(Vec::new());
    }
    let mut statement = db.prepare("WITH RECURSIVE subtree(child) AS (SELECT child_thread_id FROM thread_spawn_edges WHERE parent_thread_id = ?1 UNION SELECT edge.child_thread_id FROM thread_spawn_edges edge JOIN subtree ON edge.parent_thread_id = subtree.child) SELECT parent_thread_id, child_thread_id, status FROM thread_spawn_edges WHERE child_thread_id IN subtree AND status <> 'open'").map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([thread_id], |row| {
            Ok(AgentEdge {
                parent: row.get(0)?,
                child: row.get(1)?,
                status: row.get(2)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

pub(crate) fn apply_agent_edges(path: &Path, edges: Vec<AgentEdge>) -> Result<(), String> {
    let mut db =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)
            .map_err(|e| e.to_string())?;
    db.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| e.to_string())?;
    let tx = db.transaction().map_err(|e| e.to_string())?;
    for edge in edges {
        validate_thread_id(&edge.parent)?;
        validate_thread_id(&edge.child)?;
        tx.execute("INSERT INTO thread_spawn_edges(parent_thread_id, child_thread_id, status) VALUES (?1, ?2, ?3) ON CONFLICT(child_thread_id) DO UPDATE SET status=excluded.status WHERE parent_thread_id=excluded.parent_thread_id", rusqlite::params![edge.parent, edge.child, edge.status]).map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())
}

pub(crate) fn rollout_matches(path: &Path, id: &str) -> bool {
    path.file_name().is_some_and(|name| {
        let name = name.to_string_lossy();
        name.ends_with(&format!("-{id}.jsonl")) || name.ends_with(&format!("-{id}.jsonl.zst"))
    })
}

pub(crate) fn find_thread(dir: &Path, id: &str) -> Result<Option<PathBuf>, String> {
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_file() && rollout_matches(&entry.path(), id) {
            return Ok(Some(entry.path()));
        }
        if kind.is_dir() {
            if let Some(path) = find_thread(&entry.path(), id)? {
                return Ok(Some(path));
            }
        }
    }
    Ok(None)
}

pub(crate) fn history_base(path: &Path) -> Result<Option<String>, String> {
    use std::io::BufRead;
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let reader: Box<dyn std::io::Read> = if path.extension().is_some_and(|ext| ext == "zst") {
        Box::new(zstd::stream::read::Decoder::new(file).map_err(|e| e.to_string())?)
    } else {
        Box::new(file)
    };
    let mut reader = std::io::BufReader::new(reader);
    let mut first = String::new();
    reader.read_line(&mut first).map_err(|e| e.to_string())?;
    if first.trim().is_empty() {
        return Ok(None);
    }
    let meta: serde_json::Value =
        serde_json::from_str(&first).map_err(|_| "Could not read native Codex context metadata")?;
    Ok(meta
        .pointer("/payload/history_base/thread_id")
        .and_then(|v| v.as_str())
        .map(str::to_owned))
}
