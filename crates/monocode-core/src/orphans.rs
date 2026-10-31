//! Provider trees a previous MonoCode left running after a crash, force-quit
//! or a quit that skipped `process::shutdown_all`. Ported from the Tauri
//! harness reaper. Every provider child carries `MONOCODE_HARNESS_PARENT`, so
//! only agent CLIs whose recorded parent is gone are killed.

#[cfg(any(unix, test))]
const PARENT: &str = "MONOCODE_HARNESS_PARENT";

#[cfg(any(unix, test))]
#[derive(Debug, Clone)]
struct Snapshot {
    pid: u32,
    ppid: u32,
    args: String,
    parent: Option<u32>,
}

/// Blocking: shells out to `ps` and waits for the trees to die. Run it off the
/// UI thread, before resuming anything an orphan could still be driving.
pub fn reap() -> usize {
    #[cfg(unix)]
    {
        let ours = std::process::id();
        let pids: Vec<u32> = snapshot()
            .iter()
            .filter(|row| should_reap(row, ours, alive))
            .map(|row| row.pid)
            .collect();
        terminate(&pids);
        pids.len()
    }
    #[cfg(not(unix))]
    {
        0
    }
}

#[cfg(any(unix, test))]
fn should_reap(row: &Snapshot, ours: u32, parent_alive: impl Fn(u32) -> bool) -> bool {
    if row.pid == ours || row.pid <= 1 || row.ppid == ours {
        return false;
    }
    row.parent.is_some_and(|parent| {
        parent != ours && looks_like_agent(&row.args) && !parent_alive(parent)
    })
}

#[cfg(any(unix, test))]
/// An agent CLI we spawned, not a shell or dev server one of its tools started.
fn looks_like_agent(args: &str) -> bool {
    args.split_whitespace().any(|part| {
        let name = std::path::Path::new(part)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(part);
        matches!(
            name,
            "cursor-agent"
                | "pi-coding-agent"
                | "claude"
                | "codex"
                | "opencode"
                | "grok"
                | "omp"
                | "fx"
                | "hermes"
                | "devin"
                | "agy_acp_server.par"
                | "pi"
                | "worker-server"
                | "app-server"
        )
    })
}

#[cfg(any(unix, test))]
fn parent_from(buf: &[u8]) -> Option<u32> {
    let needle = format!("{PARENT}=");
    let start = buf
        .windows(needle.len())
        .position(|chunk| chunk == needle.as_bytes())?
        + needle.len();
    let digits = buf[start..]
        .iter()
        .take_while(|b| b.is_ascii_digit())
        .count();
    std::str::from_utf8(&buf[start..start + digits])
        .ok()?
        .parse()
        .ok()
}

#[cfg(unix)]
fn alive(pid: u32) -> bool {
    pid != 0 && unsafe { libc::kill(pid as i32, 0) == 0 }
}

#[cfg(unix)]
fn terminate(pids: &[u32]) {
    let tree_alive =
        |pid: u32| unsafe { libc::kill(pid as i32, 0) == 0 || libc::kill(-(pid as i32), 0) == 0 };
    let wait = |pids: &[u32]| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        while std::time::Instant::now() < deadline && pids.iter().any(|pid| tree_alive(*pid)) {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    };
    // Children are isolated with process_group(0), so the pid is the group id.
    for signal in [libc::SIGTERM, libc::SIGKILL] {
        let remaining: Vec<u32> = pids
            .iter()
            .copied()
            .filter(|pid| tree_alive(*pid))
            .collect();
        if remaining.is_empty() {
            return;
        }
        for pid in &remaining {
            unsafe {
                libc::kill(-(*pid as i32), signal);
                libc::kill(*pid as i32, signal);
            }
        }
        wait(&remaining);
    }
}

/// List argv for every process, then read the environment only of agent CLIs.
#[cfg(all(unix, not(target_os = "linux")))]
fn snapshot() -> Vec<Snapshot> {
    let Ok(output) = std::process::Command::new("ps")
        .args(["-axww", "-o", "pid=", "-o", "ppid=", "-o", "command="])
        .output()
    else {
        return Vec::new();
    };
    let mut rows: Vec<Snapshot> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let pid = parts.next()?.parse().ok()?;
            let ppid = parts.next()?.parse().ok()?;
            let args = parts.collect::<Vec<_>>().join(" ");
            (!args.is_empty()).then_some(Snapshot {
                pid,
                ppid,
                args,
                parent: None,
            })
        })
        .collect();
    let agents: Vec<String> = rows
        .iter()
        .filter(|row| looks_like_agent(&row.args))
        .map(|row| row.pid.to_string())
        .collect();
    if agents.is_empty() {
        return rows;
    }
    let Ok(output) = std::process::Command::new("ps")
        .args([
            "-Eww",
            "-p",
            &agents.join(","),
            "-o",
            "pid=",
            "-o",
            "command=",
        ])
        .output()
    else {
        return rows;
    };
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let line = line.trim_start();
        let Some((pid, command)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let (Ok(pid), Some(parent)) = (pid.parse::<u32>(), parent_from(command.as_bytes())) else {
            continue;
        };
        if let Some(row) = rows.iter_mut().find(|row| row.pid == pid) {
            row.parent = Some(parent);
        }
    }
    rows
}

#[cfg(target_os = "linux")]
fn snapshot() -> Vec<Snapshot> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let pid: u32 = entry.file_name().to_str()?.parse().ok()?;
            let dir = entry.path();
            let cmdline = std::fs::read(dir.join("cmdline")).ok()?;
            let args = String::from_utf8_lossy(&cmdline).replace('\0', " ");
            if args.trim().is_empty() {
                return None;
            }
            let stat = std::fs::read_to_string(dir.join("stat")).ok()?;
            // The command name may contain spaces; fields resume after ')'.
            let ppid = stat
                .rsplit_once(')')?
                .1
                .split_whitespace()
                .nth(1)?
                .parse()
                .ok()?;
            let parent = looks_like_agent(&args)
                .then(|| std::fs::read(dir.join("environ")).ok())
                .flatten()
                .and_then(|env| parent_from(&env));
            Some(Snapshot {
                pid,
                ppid,
                args,
                parent,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(args: &str, parent: Option<u32>) -> Snapshot {
        Snapshot {
            pid: 500,
            ppid: 1,
            args: args.into(),
            parent,
        }
    }

    #[test]
    fn only_agents_whose_monocode_is_gone_are_reaped() {
        let dead = |_| false;
        assert!(should_reap(
            &row("/usr/local/bin/opencode serve", Some(42)),
            7,
            dead
        ));
        assert!(!should_reap(
            &row("/usr/local/bin/opencode serve", Some(42)),
            7,
            |_| true
        ));
        assert!(!should_reap(
            &row("/usr/local/bin/opencode serve", Some(7)),
            7,
            dead
        ));
        assert!(!should_reap(&row("npm run dev", Some(42)), 7, dead));
        assert!(!should_reap(&row("/usr/local/bin/claude", None), 7, dead));
    }

    #[test]
    fn reads_the_parent_from_an_environment_dump() {
        assert_eq!(
            parent_from(b"codex app-server PATH=/usr/bin MONOCODE_HARNESS_PARENT=21504 HOME=/tmp"),
            Some(21504)
        );
        assert_eq!(parent_from(b"\0MONOCODE_HARNESS_PARENT=9\0"), Some(9));
        assert_eq!(parent_from(b"codex app-server"), None);
    }

    /// Stand-in agent for the reap test. macOS hides the environment of
    /// platform binaries such as /bin/sleep, so the test runs this binary.
    #[test]
    #[ignore]
    fn sleeping_agent() {
        if std::env::var_os("MONOCODE_REAP_TEST").is_some() {
            std::thread::sleep(std::time::Duration::from_secs(30));
        }
    }

    #[cfg(unix)]
    #[test]
    fn reaps_a_marked_orphan_and_spares_our_own() {
        use std::os::unix::process::CommandExt;
        let dir = tempfile::tempdir().unwrap();
        let agent = dir.path().join("opencode");
        std::os::unix::fs::symlink(std::env::current_exe().unwrap(), &agent).unwrap();
        let args = ["--exact", "orphans::tests::sleeping_agent", "--ignored"];
        // The shell exits at once, so the agent is reparented like a real orphan.
        let output = std::process::Command::new("/bin/sh")
            .args(["-c", "\"$0\" \"$@\" >/dev/null 2>&1 & echo $!"])
            .arg(&agent)
            .args(args)
            .env(PARENT, "2147483646")
            .env("MONOCODE_REAP_TEST", "1")
            .process_group(0)
            .output()
            .unwrap();
        let orphan: i32 = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse()
            .unwrap();
        let mut ours = std::process::Command::new(&agent)
            .args(args)
            .env(PARENT, std::process::id().to_string())
            .env("MONOCODE_REAP_TEST", "1")
            .stdout(std::process::Stdio::null())
            .process_group(0)
            .spawn()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
        let reaped = reap();
        let orphan_alive = unsafe { libc::kill(orphan, 0) } == 0;
        let ours_alive = matches!(ours.try_wait(), Ok(None));
        let _ = ours.kill();
        let _ = ours.wait();
        unsafe { libc::kill(orphan, libc::SIGKILL) };
        assert!(reaped >= 1);
        assert!(!orphan_alive);
        assert!(ours_alive);
    }
}
