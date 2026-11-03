//! `!` commands from the session composer: run once through the user's shell
//! in the session's folder, outside any agent turn.

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::{Mutex, MutexGuard};
use std::thread;

use serde::Serialize;

use crate::fs::expand_home;

/// Bytes kept per stream; Claude Code cuts its own `!` output at the same length.
const OUTPUT_LIMIT: usize = 30_000;

/// Run id → pid of each command still running.
static RUNNING: Mutex<Vec<(String, u32)>> = Mutex::new(Vec::new());

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ShellCommandOutput {
    stdout: String,
    stderr: String,
    /// `None` when a signal ended it.
    exit_code: Option<i32>,
    truncated: bool,
    stopped: bool,
}

#[tauri::command]
pub async fn shell_command_run(
    id: String,
    cwd: String,
    command: String,
) -> Result<ShellCommandOutput, String> {
    tauri::async_runtime::spawn_blocking(move || run(&id, &cwd, &command))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn shell_command_stop(id: String) {
    if let Some(pid) = take(&id) {
        crate::harness::terminate(pid);
    }
}

pub(crate) fn kill_all() {
    let pids: Vec<u32> = running().drain(..).map(|(_, pid)| pid).collect();
    crate::harness::terminate_all(&pids);
}

fn running() -> MutexGuard<'static, Vec<(String, u32)>> {
    RUNNING.lock().unwrap_or_else(|e| e.into_inner())
}

fn take(id: &str) -> Option<u32> {
    let mut running = running();
    let index = running.iter().position(|(run, _)| run == id)?;
    Some(running.swap_remove(index).1)
}

fn run(id: &str, cwd: &str, command: &str) -> Result<ShellCommandOutput, String> {
    let workdir = expand_home(cwd);
    if !workdir.is_dir() {
        return Err(format!("{cwd} is not a folder"));
    }
    let (shell, args) = crate::pty::default_shell();
    let mut cmd = Command::new(&shell);
    cmd.args(args);
    command_args(&mut cmd, &shell, command);
    cmd.current_dir(&workdir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // The same environment and process group an agent's CLI gets, so a stop
    // reaches everything the command started.
    crate::harness::prepare_child(&mut cmd, &shell);
    let mut child = {
        let _reservation = crate::worktree_lifecycle::reserve_spawn(&workdir)?;
        crate::harness::spawn_managed(&mut cmd)
            .map_err(|e| format!("Failed to run {shell}: {e}"))?
    };
    running().push((id.to_string(), child.id()));
    let stdout = child.stdout.take().map(read_in_background);
    let stderr = child.stderr.take().map(read_in_background);
    let status = child.wait();
    let joined = |reader: Option<thread::JoinHandle<(String, bool)>>| {
        reader.and_then(|r| r.join().ok()).unwrap_or_default()
    };
    // Something it left running in the background can hold the pipes open
    // after the shell exits; it stays stoppable until they close.
    let (stdout, stdout_cut) = joined(stdout);
    let (stderr, stderr_cut) = joined(stderr);
    let stopped = take(id).is_none();
    Ok(ShellCommandOutput {
        stdout,
        stderr,
        exit_code: status.map_err(|e| e.to_string())?.code(),
        truncated: stdout_cut || stderr_cut,
        stopped,
    })
}

#[cfg(not(windows))]
fn command_args(cmd: &mut Command, _shell: &str, command: &str) {
    cmd.arg("-c").arg(command);
}

#[cfg(windows)]
fn command_args(cmd: &mut Command, shell: &str, command: &str) {
    use std::os::windows::process::CommandExt;
    let name = std::path::Path::new(shell)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or(shell)
        .to_ascii_lowercase();
    if name == "powershell" || name == "pwsh" {
        cmd.arg("-Command").arg(command);
    } else {
        // cmd.exe parses its own command line; quoting it would change it.
        cmd.arg("/C").raw_arg(command);
    }
}

fn read_in_background(pipe: impl Read + Send + 'static) -> thread::JoinHandle<(String, bool)> {
    thread::spawn(move || read_capped(pipe, OUTPUT_LIMIT))
}

/// The first `limit` bytes, and whether there was more. Reads to the end
/// either way, so a chatty command never stalls on a full pipe.
fn read_capped(mut reader: impl Read, limit: usize) -> (String, bool) {
    let mut kept = Vec::new();
    let mut cut = false;
    let mut buf = [0u8; 8192];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let room = limit.saturating_sub(kept.len());
                kept.extend_from_slice(&buf[..n.min(room)]);
                cut |= n > room;
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    (String::from_utf8_lossy(&kept).into_owned(), cut)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_folder() -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("monocode-shell-command-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn runs_in_the_folder_and_reports_both_streams_and_the_exit_code() {
        let dir = temp_folder();
        let command = if cfg!(windows) {
            "cd & echo oops 1>&2 & exit /b 3"
        } else {
            "pwd; echo oops >&2; exit 3"
        };
        let output = run("test-run", dir.to_str().unwrap(), command).unwrap();
        let name = dir.file_name().unwrap().to_str().unwrap();
        assert!(output.stdout.contains(name), "{output:?}");
        assert_eq!(output.stderr.trim(), "oops");
        assert_eq!(output.exit_code, Some(3));
        assert!(!output.stopped && !output.truncated);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_missing_folder_is_an_error_not_a_run_somewhere_else() {
        let dir = temp_folder().join("gone");
        assert!(run("test-missing", dir.to_str().unwrap(), "echo hi").is_err());
    }

    /// Runs `command`, stops it once `ready(pid)`, and returns what it printed.
    #[cfg(unix)]
    fn stop_when(id: &str, command: &str, ready: fn(u32) -> bool) -> ShellCommandOutput {
        use std::time::{Duration, Instant};
        let dir = temp_folder();
        let cwd = dir.to_str().unwrap().to_string();
        let (run_id, command) = (id.to_string(), command.to_string());
        let started = Instant::now();
        let runner = thread::spawn(move || run(&run_id, &cwd, &command));
        loop {
            let pid = running().iter().find(|(run, _)| run == id).map(|r| r.1);
            if pid.is_some_and(ready) {
                break;
            }
            assert!(started.elapsed() < Duration::from_secs(20), "never ready");
            thread::sleep(Duration::from_millis(10));
        }
        shell_command_stop(id.into());
        let output = runner.join().unwrap().unwrap();
        // Well short of the `sleep 30` it was running.
        assert!(started.elapsed() < Duration::from_secs(25));
        std::fs::remove_dir_all(dir).unwrap();
        output
    }

    #[cfg(unix)]
    #[test]
    fn stop_ends_the_command_and_says_so() {
        let output = stop_when("test-stop", "sleep 30", |_| true);
        assert!(output.stopped);
        assert_eq!(output.exit_code, None);
    }

    #[cfg(unix)]
    #[test]
    fn stop_reaches_what_it_left_running_in_the_background() {
        // Ready once the shell itself has exited and been reaped.
        let output = stop_when("test-background", "sleep 30 & echo started", |pid| unsafe {
            libc::kill(pid as i32, 0) != 0
        });
        assert!(output.stopped);
        assert_eq!(output.exit_code, Some(0));
        assert_eq!(output.stdout, "started\n");
    }

    #[test]
    fn output_past_the_limit_is_dropped_but_still_read() {
        let bytes = vec![b'x'; 20_000];
        assert_eq!(read_capped(&bytes[..], 10), ("x".repeat(10), true));
        assert_eq!(read_capped(&bytes[..], 20_000), ("x".repeat(20_000), false));
    }
}
