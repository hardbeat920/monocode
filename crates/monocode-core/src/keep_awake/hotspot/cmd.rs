//! Runs a command with a time limit, keeping stdout and stderr. A hung
//! `networksetup` must not stall the feature: while one runs, nothing else
//! probes, joins or leaves.
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, PartialEq, Eq)]
pub enum Ran {
    /// Exited; `ok` is a zero status. `out` is stdout then stderr.
    Exited { ok: bool, out: String },
    /// Still running at the limit, so it was killed.
    TimedOut,
    /// Couldn't start.
    NotRun,
}
impl Ran {
    /// Stdout and stderr of a zero exit.
    pub fn ok(self) -> Option<String> {
        match self {
            Self::Exited { ok: true, out } => Some(out),
            _ => None,
        }
    }
}

pub fn run(program: &str, args: &[&str], limit: Duration) -> Ran {
    let Ok(mut child) = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    else {
        return Ran::NotRun;
    };
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut text = String::new();
            if let Some(mut pipe) = pipe {
                let mut bytes = Vec::new();
                let _ = pipe.read_to_end(&mut bytes);
                text = String::from_utf8_lossy(&bytes).into_owned();
            }
            text
        })
    };
    let stdout = drain(child.stdout.take().map(|p| Box::new(p) as _));
    let stderr = drain(child.stderr.take().map(|p| Box::new(p) as _));
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if start.elapsed() < limit => std::thread::sleep(Duration::from_millis(50)),
            _ => break None,
        }
    };
    let Some(status) = status else {
        let _ = child.kill();
        let _ = child.wait();
        return Ran::TimedOut;
    };
    let mut out = stdout.join().unwrap_or_default();
    let err = stderr.join().unwrap_or_default();
    if !err.trim().is_empty() {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&err);
    }
    Ran::Exited {
        ok: status.success(),
        out,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hung_command_times_out() {
        let start = Instant::now();
        assert_eq!(
            run("/bin/sleep", &["5"], Duration::from_millis(200)),
            Ran::TimedOut
        );
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn keeps_stderr_and_the_status() {
        let ran = run(
            "/bin/sh",
            &["-c", "echo out; echo 'Could not find network X.' >&2; exit 0"],
            Duration::from_secs(5),
        );
        assert_eq!(
            ran,
            Ran::Exited {
                ok: true,
                out: "out\nCould not find network X.\n".into()
            }
        );
        assert!(matches!(
            run("/bin/sh", &["-c", "exit 3"], Duration::from_secs(5)),
            Ran::Exited { ok: false, .. }
        ));
        assert_eq!(
            run("/nonexistent/tool", &[], Duration::from_secs(1)),
            Ran::NotRun
        );
    }
}
