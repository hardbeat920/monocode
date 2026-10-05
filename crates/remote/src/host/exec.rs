//! Node's `execFile` with `timeout` and `maxBuffer`, as the TypeScript host
//! used it to run `git`, `tailscale`, `launchctl`, `systemctl`, `ps`, and
//! PowerShell.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct ExecOptions {
    pub cwd: Option<PathBuf>,
    pub timeout: Duration,
    /// Largest stdout or stderr kept, in bytes. More fails the call.
    pub max_buffer: usize,
    /// Replaces the environment when set.
    pub env: Option<Vec<(OsString, OsString)>>,
    /// Written to the child's stdin, which is then closed.
    pub stdin: Option<Vec<u8>>,
}

impl Default for ExecOptions {
    fn default() -> Self {
        Self {
            cwd: None,
            timeout: Duration::from_secs(30),
            max_buffer: 1024 * 1024,
            env: None,
            stdin: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecOutput {
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecError {
    /// Node's `error.message`: `Command failed: <command>\n<stderr>`.
    pub message: String,
    pub stdout: String,
    pub stderr: String,
    /// The exit code, when the child exited on its own.
    pub code: Option<i32>,
    /// The program could not be started, as Node's `ENOENT`.
    pub not_found: bool,
}

impl std::fmt::Display for ExecError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl From<ExecError> for String {
    fn from(error: ExecError) -> Self {
        error.message
    }
}

fn collect(
    mut source: impl Read + Send + 'static,
    limit: usize,
    exceeded: Arc<AtomicBool>,
) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut kept = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            match source.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    if kept.len() + count > limit {
                        exceeded.store(true, Ordering::SeqCst);
                        kept.extend_from_slice(&chunk[..limit.saturating_sub(kept.len())]);
                    } else {
                        kept.extend_from_slice(&chunk[..count]);
                    }
                }
            }
        }
        kept
    })
}

/// Runs `program` with `args` and returns its output, failing on a non-zero
/// exit, a timeout, or output past `max_buffer`.
pub fn exec(program: &str, args: &[&str], options: ExecOptions) -> Result<ExecOutput, ExecError> {
    let display = std::iter::once(program)
        .chain(args.iter().copied())
        .collect::<Vec<_>>()
        .join(" ");
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(if options.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(cwd) = &options.cwd {
        command.current_dir(cwd);
    }
    if let Some(env) = &options.env {
        command
            .env_clear()
            .envs(env.iter().map(|(key, value)| (key, value)));
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command.spawn().map_err(|error| ExecError {
        message: format!("spawn {program} {}", error),
        stdout: String::new(),
        stderr: String::new(),
        code: None,
        not_found: error.kind() == std::io::ErrorKind::NotFound,
    })?;
    if let (Some(input), Some(mut stdin)) = (options.stdin, child.stdin.take()) {
        std::thread::spawn(move || {
            // A child that exits early closes the pipe; its exit reports why.
            let _ = stdin.write_all(&input);
        });
    }
    let exceeded = Arc::new(AtomicBool::new(false));
    let stdout = collect(
        child.stdout.take().expect("stdout is piped"),
        options.max_buffer,
        exceeded.clone(),
    );
    let stderr = collect(
        child.stderr.take().expect("stderr is piped"),
        options.max_buffer,
        exceeded.clone(),
    );
    let deadline = Instant::now() + options.timeout;
    let mut killed = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(_) => break None,
        }
        if Instant::now() >= deadline || exceeded.load(Ordering::SeqCst) {
            #[cfg(unix)]
            // SAFETY: The child has its own group. Descendants must release their output pipes too.
            unsafe {
                libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
            }
            let _ = child.kill();
            killed = true;
            break child.wait().ok();
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let stdout = String::from_utf8_lossy(&stdout.join().unwrap_or_default()).into_owned();
    let stderr = String::from_utf8_lossy(&stderr.join().unwrap_or_default()).into_owned();
    let code = status.and_then(|status| status.code());
    if exceeded.load(Ordering::SeqCst) {
        return Err(ExecError {
            message: "stdout maxBuffer length exceeded".into(),
            stdout,
            stderr,
            code,
            not_found: false,
        });
    }
    if killed || code != Some(0) {
        return Err(ExecError {
            message: format!("Command failed: {display}\n{stderr}"),
            stdout,
            stderr,
            code: if killed { None } else { code },
            not_found: false,
        });
    }
    Ok(ExecOutput { stdout, stderr })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn reports_output_failures_timeouts_and_large_output() {
        let ok = exec(
            "sh",
            &["-c", "printf hi; printf err >&2"],
            ExecOptions::default(),
        )
        .unwrap();
        assert_eq!(
            ok,
            ExecOutput {
                stdout: "hi".into(),
                stderr: "err".into()
            }
        );
        let failed = exec(
            "sh",
            &["-c", "echo nope >&2; exit 3"],
            ExecOptions::default(),
        )
        .unwrap_err();
        assert_eq!(failed.code, Some(3));
        assert_eq!(
            failed.message,
            "Command failed: sh -c echo nope >&2; exit 3\nnope\n"
        );
        let started = Instant::now();
        let slow = exec(
            "sh",
            &["-c", "sleep 5"],
            ExecOptions {
                timeout: Duration::from_millis(100),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(slow.message.starts_with("Command failed"));
        let large = exec(
            "sh",
            &["-c", "yes | head -c 100000"],
            ExecOptions {
                max_buffer: 1000,
                ..Default::default()
            },
        )
        .unwrap_err();
        assert_eq!(large.message, "stdout maxBuffer length exceeded");
        assert!(
            exec("monocode-no-such-program", &[], ExecOptions::default())
                .unwrap_err()
                .not_found
        );
        let echoed = exec(
            "cat",
            &[],
            ExecOptions {
                stdin: Some(b"piped".to_vec()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(echoed.stdout, "piped");
    }
}
