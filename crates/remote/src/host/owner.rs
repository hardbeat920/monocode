//! Port of host/owner.ts.
//!
//! One host owns a data directory at a time. The owner holds an exclusive
//! SQLite transaction on `owner.db` for as long as it runs, which the
//! TypeScript host also did, so a Node host and this one exclude each other.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use rusqlite::Connection;
use serde_json::Value;

use super::exec::{ExecOptions, exec};

const ALREADY_OWNED: &str = "A host already owns this data directory";

/// Held while this process owns the data directory. Dropping it releases
/// the lock.
pub struct HostOwner {
    db: Option<Connection>,
    path: PathBuf,
}

impl HostOwner {
    pub fn release(mut self) {
        self.release_now();
    }

    fn release_now(&mut self) {
        if let Some(db) = self.db.take() {
            let _ = std::fs::remove_file(&self.path);
            drop(db);
        }
    }
}

impl Drop for HostOwner {
    fn drop(&mut self) {
        self.release_now();
    }
}

/// Whether `pid` names a running process, as `process.kill(pid, 0)`.
fn process_alive(pid: i64) -> bool {
    #[cfg(unix)]
    {
        let Ok(pid) = libc::pid_t::try_from(pid) else {
            return false;
        };
        // SAFETY: signal 0 only checks that the process exists.
        if unsafe { libc::kill(pid, 0) } == 0 {
            return true;
        }
        std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
    }
    #[cfg(windows)]
    {
        exec(
            "tasklist",
            &["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"],
            ExecOptions {
                timeout: Duration::from_secs(5),
                ..Default::default()
            },
        )
        .map(|output| output.stdout.contains(&format!("\"{pid}\"")))
        .unwrap_or(true)
    }
}

/// The command line of a running process.
fn process_command(pid: i64) -> Result<String, String> {
    if cfg!(windows) {
        super::windows::run_powershell(&format!(
            "$p = Get-CimInstance Win32_Process -Filter 'ProcessId = {pid}'; if ($null -eq $p.CommandLine) {{ throw 'Cannot inspect host lock owner' }}; $p.CommandLine"
        ))
    } else {
        Ok(exec(
            "ps",
            &["-ww", "-p", &pid.to_string(), "-o", "args="],
            ExecOptions {
                timeout: Duration::from_secs(5),
                ..Default::default()
            },
        )?
        .stdout)
    }
}

fn looks_like_host(command: &str) -> bool {
    static HOST: OnceLock<regex::Regex> = OnceLock::new();
    HOST.get_or_init(|| regex::Regex::new(r"monocode-host.*\bserve\b").expect("valid pattern"))
        .is_match(command)
}

fn write_lock(path: &Path) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let text = serde_json::json!({ "version": 2, "pid": std::process::id() }).to_string();
    std::io::Write::write_all(&mut options.open(path)?, text.as_bytes())
}

/// Kernel-backed SQLite locking survives neither crashes nor PID reuse. The
/// PID file remains for compatibility with hosts installed before this lock.
pub fn acquire_host_owner(directory: &Path) -> Result<HostOwner, String> {
    let db = Connection::open(directory.join("owner.db")).map_err(|error| error.to_string())?;
    // Fail at once instead of waiting for the other owner.
    db.busy_timeout(Duration::ZERO)
        .map_err(|error| error.to_string())?;
    if db.execute_batch("BEGIN EXCLUSIVE").is_err() {
        return Err(ALREADY_OWNED.into());
    }
    let path = directory.join("owner.lock");
    if let Ok(contents) = std::fs::read_to_string(&path) {
        // Version 2 owners hold the SQLite lock we just acquired. Their leftover
        // PID file is therefore stale, regardless of which process owns that PID.
        let modern = serde_json::from_str::<Value>(&contents)
            .ok()
            .is_some_and(|value| value.get("version").and_then(Value::as_f64) == Some(2.0));
        if !modern {
            let pid = super::js::number_from_str(&contents);
            if !super::js::is_safe_integer_f64(pid) || pid < 1.0 {
                return Err(
                    "Host lock is incomplete; inspect owner.lock before removing it".into(),
                );
            }
            let pid = pid as i64;
            if process_alive(pid) {
                // Never treat an unresponsive host as dead. For legacy locks,
                // check the process command, conservatively refusing if it
                // cannot be read.
                let command = process_command(pid)?;
                if command.trim().is_empty() || looks_like_host(&command) {
                    return Err(ALREADY_OWNED.into());
                }
            }
        }
    }
    // Old clients cannot mistake this for a dead PID and take ownership.
    write_lock(&path).map_err(|error| error.to_string())?;
    Ok(HostOwner { db: Some(db), path })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_simultaneous_owners_and_recovers_a_stale_modern_pid_lock() {
        let directory = crate::host::store::tests::temporary("monocode-owner-");
        let owner = acquire_host_owner(directory.path()).unwrap();
        assert_eq!(
            acquire_host_owner(directory.path()).err().unwrap(),
            "A host already owns this data directory"
        );
        owner.release();
        assert!(!directory.path().join("owner.lock").exists());
        std::fs::write(
            directory.path().join("owner.lock"),
            serde_json::json!({ "version": 2, "pid": std::process::id() }).to_string(),
        )
        .unwrap();
        let owner = acquire_host_owner(directory.path()).unwrap();
        drop(owner);
    }

    #[test]
    fn recovers_a_legacy_pid_that_now_belongs_to_another_program() {
        let directory = crate::host::store::tests::temporary("monocode-owner-");
        // A stale legacy PID now belongs to this unrelated test process.
        std::fs::write(
            directory.path().join("owner.lock"),
            std::process::id().to_string(),
        )
        .unwrap();
        let owner = acquire_host_owner(directory.path()).unwrap();
        let lock: Value = serde_json::from_str(
            &std::fs::read_to_string(directory.path().join("owner.lock")).unwrap(),
        )
        .unwrap();
        assert_eq!(lock["version"], 2);
        drop(owner);
        std::fs::write(directory.path().join("owner.lock"), "garbage").unwrap();
        assert!(
            acquire_host_owner(directory.path())
                .err()
                .unwrap()
                .contains("incomplete")
        );
        assert!(looks_like_host(
            "node /x/monocode-host.mjs serve --data-dir y"
        ));
        assert!(!looks_like_host("node unrelated.mjs serve"));
    }
}
