//! Real PTYs, real shells: what a page reload (or macOS rebuilding a crashed
//! web process) does to a running terminal, and what closing a window or
//! quitting does to its processes. Shells and programs that are not
//! installed are skipped with a note, so `cargo test` stays green on a bare
//! machine; CI images with bash, zsh and vim run all of it.

use super::*;
use std::sync::Condvar;
use std::time::Instant;

const WAIT: Duration = Duration::from_secs(10);
const COLS: u16 = 80;
const ROWS: u16 = 24;

#[derive(Default)]
struct Capture {
    data: Mutex<HashMap<String, Vec<u8>>>,
    exits: Mutex<HashMap<String, Option<i32>>>,
    changed: Condvar,
}

impl PtyOutput for Capture {
    fn data(&self, id: &str, bytes: &[u8]) {
        self.data
            .lock()
            .unwrap()
            .entry(id.to_string())
            .or_default()
            .extend_from_slice(bytes);
        self.changed.notify_all();
    }

    fn exit(&self, id: &str, code: Option<i32>) {
        self.exits.lock().unwrap().insert(id.to_string(), code);
        self.changed.notify_all();
    }
}

impl Capture {
    fn len(&self, id: &str) -> usize {
        self.data.lock().unwrap().get(id).map_or(0, Vec::len)
    }

    fn since(&self, id: &str, from: usize) -> Vec<u8> {
        self.data
            .lock()
            .unwrap()
            .get(id)
            .map(|out| out[from.min(out.len())..].to_vec())
            .unwrap_or_default()
    }

    /// Offset just past the first `needle` at or after `from`.
    fn wait_for(&self, id: &str, from: usize, needle: &[u8]) -> usize {
        let deadline = Instant::now() + WAIT;
        let mut data = self.data.lock().unwrap();
        loop {
            let out = data.get(id).map(Vec::as_slice).unwrap_or_default();
            let tail = &out[from.min(out.len())..];
            if let Some(at) = find(tail, needle) {
                return from + at + needle.len();
            }
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(
                !left.is_zero(),
                "{id}: no {:?} after offset {from}; got {:?}",
                String::from_utf8_lossy(needle),
                String::from_utf8_lossy(tail),
            );
            data = self.changed.wait_timeout(data, left).unwrap().0;
        }
    }

    /// Wait until output stops for `quiet`, so a test can assert on what
    /// did not happen.
    fn settle(&self, id: &str, quiet: Duration) {
        let deadline = Instant::now() + WAIT;
        let mut last = self.len(id);
        loop {
            thread::sleep(quiet);
            let now = self.len(id);
            if now == last || Instant::now() > deadline {
                return;
            }
            last = now;
        }
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn program(name: &str) -> Option<String> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
        .map(|path| path.to_string_lossy().into_owned())
}

/// Shells without the user's rc files, so prompts and modes are the
/// shell's own defaults. Both turn bracketed paste on at every prompt, bash
/// from 5.1 (macOS still ships 3.2 as /bin/bash).
fn shells() -> Vec<(&'static str, (String, Vec<String>))> {
    let mut found = Vec::new();
    match program("bash").filter(|bash| bash_has_bracketed_paste(bash)) {
        Some(bash) => found.push((
            "bash",
            (
                bash,
                vec!["--norc".into(), "--noprofile".into(), "-i".into()],
            ),
        )),
        None => eprintln!("skipping bash: not installed, or older than 5.1"),
    }
    match program("zsh") {
        Some(zsh) => found.push(("zsh", (zsh, vec!["-f".into(), "-i".into()]))),
        None => eprintln!("skipping zsh: not installed"),
    }
    found
}

fn bash_has_bracketed_paste(bash: &str) -> bool {
    std::process::Command::new(bash)
        .args(["-c", "echo ${BASH_VERSINFO[0]} ${BASH_VERSINFO[1]}"])
        .output()
        .ok()
        .and_then(|out| {
            let version = String::from_utf8(out.stdout).ok()?;
            let mut parts = version.split_whitespace().map(str::parse::<u32>);
            Some((parts.next()?.ok()?, parts.next()?.ok()?))
        })
        .is_some_and(|version| version >= (5, 1))
}

/// Any shell the reload tests can drive.
fn shell() -> Option<(String, Vec<String>)> {
    let found = shells().into_iter().next().map(|(_, shell)| shell);
    if found.is_none() {
        eprintln!("skipping: no bash 5.1+ or zsh");
    }
    found
}

const PROMPT_READY: &[u8] = b"\x1b[?2004h";

struct Rig {
    host: Arc<PtyHost>,
    out: Arc<Capture>,
    dir: std::path::PathBuf,
}

impl Rig {
    fn new() -> Self {
        Self {
            host: Arc::new(PtyHost::new()),
            out: Arc::new(Capture::default()),
            dir: {
                let dir = std::env::temp_dir()
                    .join(format!("monocode-pty-test-{}", uuid::Uuid::new_v4()));
                std::fs::create_dir_all(&dir).unwrap();
                dir
            },
        }
    }

    fn open(&self, id: &str, owner: &str, shell: &(String, Vec<String>)) -> PtyAttach {
        self.host
            .open(
                self.out.clone(),
                owner,
                id.to_string(),
                &self.dir.to_string_lossy(),
                COLS,
                ROWS,
                shell.clone(),
            )
            .unwrap()
    }

    /// Start a shell and wait for its first prompt.
    fn start(&self, id: &str, owner: &str, shell: &(String, Vec<String>)) -> u32 {
        let attach = self.open(id, owner, shell);
        assert!(!attach.reattached);
        assert_eq!(attach.restore, "");
        self.out.wait_for(id, 0, PROMPT_READY);
        self.out.settle(id, Duration::from_millis(100));
        self.pid(id)
    }

    fn pid(&self, id: &str) -> u32 {
        self.host.get(id).expect("terminal is running").pid
    }

    fn write(&self, id: &str, data: &[u8]) {
        self.host.write(id, data).unwrap();
    }

    /// Type a command, wait for the shell to come back to a prompt, and
    /// return what it printed.
    fn run(&self, id: &str, command: &str) -> String {
        let from = self.out.len(id);
        self.write(id, format!("{command}\r").as_bytes());
        let end = self.out.wait_for(id, from, PROMPT_READY);
        String::from_utf8_lossy(&self.out.since(id, from)[..end - from]).into_owned()
    }

    fn foreground(&self, id: &str) -> i32 {
        let fd = self.host.get(id).unwrap().master_fd;
        let mut pgrp: libc::pid_t = 0;
        assert_eq!(unsafe { libc::ioctl(fd, libc::TIOCGPGRP, &mut pgrp) }, 0);
        pgrp
    }

    /// Run a foreground job and wait until it owns the terminal.
    fn start_job(&self, id: &str, command: &str) -> i32 {
        let shell = self.pid(id) as i32;
        self.write(id, format!("{command}\r").as_bytes());
        wait_until(
            || self.foreground(id) != shell,
            "job never took the terminal",
        );
        self.foreground(id)
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.host.kill_all();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn wait_until(mut done: impl FnMut() -> bool, what: &str) {
    let deadline = Instant::now() + WAIT;
    while !done() {
        assert!(Instant::now() < deadline, "{what}");
        thread::sleep(Duration::from_millis(20));
    }
}

/// Exited, or a zombie its wait thread has yet to collect.
fn gone(pid: i32) -> bool {
    if unsafe { libc::kill(pid, 0) } != 0 {
        return true;
    }
    std::process::Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .map(|out| {
            let stat = String::from_utf8_lossy(&out.stdout);
            stat.trim().is_empty() || stat.trim_start().starts_with('Z')
        })
        .unwrap_or(false)
}

fn group_gone(pgrp: i32) -> bool {
    unsafe { libc::kill(-pgrp, 0) != 0 }
}

#[test]
fn reloads_keep_the_shell_its_input_modes_and_its_job() {
    for (name, shell) in shells() {
        let rig = Rig::new();
        let pid = rig.start("t", "main", &shell);

        // Reload a few times at the same size, as a reload restores the
        // view's old dimensions. Same shell, and the new view learns the
        // prompt has bracketed paste on.
        for round in 0..3 {
            let attach = rig.open("t", "main", &shell);
            assert!(attach.reattached, "{name}: round {round}");
            assert_eq!(attach.restore, "\x1b[?2004h", "{name}: round {round}");
            assert_eq!(rig.pid("t"), pid, "{name}: round {round}");
        }

        // Input still reaches the same shell.
        let out = rig.run("t", "echo mark-$((6*7))");
        assert!(out.contains("mark-42"), "{name}: {out:?}");

        // A paste the restored view wraps stays pending, two lines and all,
        // until Enter. This is what a view at default modes gets wrong: it
        // sends the lines bare and the shell runs each one as it lands.
        let from = rig.out.len("t");
        rig.write(
            "t",
            b"\x1b[200~echo pasted-$((1+1))\recho pasted-$((2+1))\r\x1b[201~",
        );
        rig.out.settle("t", Duration::from_millis(300));
        let pending = String::from_utf8_lossy(&rig.out.since("t", from)).into_owned();
        assert!(
            !pending.contains("pasted-2\r\n") && !pending.contains("pasted-3\r\n"),
            "{name}: a bracketed paste ran before Enter: {pending:?}",
        );
        let out = rig.run("t", "");
        assert!(out.contains("pasted-2\r\n"), "{name}: {out:?}");
        assert!(out.contains("pasted-3\r\n"), "{name}: {out:?}");

        let from = rig.out.len("t");
        rig.write("t", b"echo bare-$((1+1))\recho bare-$((2+1))\r");
        rig.out.wait_for("t", from, b"bare-3\r\n");
        assert!(find(&rig.out.since("t", from), b"bare-2\r\n").is_some());
        rig.out.settle("t", Duration::from_millis(100));

        // A foreground job survives reloads, and the view learns the shell
        // turned bracketed paste off while the job runs.
        let job = rig.start_job("t", "sleep 1000");
        rig.out.settle("t", Duration::from_millis(100));
        for round in 0..3 {
            let attach = rig.open("t", "main", &shell);
            assert!(attach.reattached);
            assert_eq!(attach.restore, "", "{name}: round {round}");
            assert_eq!(rig.pid("t"), pid);
            assert_eq!(rig.foreground("t"), job, "{name}: round {round}");
            assert!(!group_gone(job), "{name}: the job died on reload {round}");
        }

        // Ctrl+C reaches the job, not the shell.
        let from = rig.out.len("t");
        rig.write("t", b"\x03");
        rig.out.wait_for("t", from, PROMPT_READY);
        wait_until(|| group_gone(job), "Ctrl+C left the job running");
        assert_eq!(rig.pid("t"), pid);
        let out = rig.run("t", "echo after-$((5+5))");
        assert!(out.contains("after-10"), "{name}: {out:?}");
    }
}

#[test]
fn a_reloaded_tui_gets_its_modes_back_and_redraws() {
    let Some(shell) = shell() else { return };
    let Some(vim) = program("vim") else {
        eprintln!("skipping vim: not installed");
        return;
    };
    let rig = Rig::new();
    let pid = rig.start("t", "main", &shell);

    let from = rig.out.len("t");
    let editor = rig.start_job("t", &format!("{vim} -u NONE -N -i NONE notes.txt"));
    rig.out.wait_for("t", from, b"\x1b[?1049h");
    rig.out.settle("t", Duration::from_millis(300));

    for round in 0..3 {
        let from = rig.out.len("t");
        let attach = rig.open("t", "main", &shell);
        assert!(attach.reattached);
        let restore = &attach.restore;
        // The alternate screen first, so the redraw paints into it.
        assert!(
            restore.starts_with("\x1b[?1049h"),
            "round {round}: {restore:?}"
        );
        for mode in ["\x1b[?1h", "\x1b=", "\x1b[?2004h", "\x1b[?1004h"] {
            assert!(
                restore.contains(mode),
                "round {round}: no {mode:?} in {restore:?}"
            );
        }
        // Same size, and still a full repaint: clear plus the empty-line tildes.
        rig.out.wait_for("t", from, b"\x1b[2J");
        rig.out.wait_for("t", from, b"~");
        rig.out.settle("t", Duration::from_millis(200));
        assert_eq!(rig.foreground("t"), editor);
        assert_eq!(rig.pid("t"), pid);
    }

    // Keys still reach the editor and it still writes.
    let from = rig.out.len("t");
    rig.write("t", b"ihello from vim\x1b:wq\r");
    rig.out.wait_for("t", from, PROMPT_READY);
    wait_until(|| group_gone(editor), "vim did not quit");
    let saved = std::fs::read_to_string(rig.dir.join("notes.txt")).unwrap();
    assert_eq!(saved, "hello from vim\n");

    // Back at the prompt there is nothing of vim's left to restore.
    let attach = rig.open("t", "main", &shell);
    assert_eq!(attach.restore, "\x1b[?2004h");
}

#[test]
fn a_reattached_terminal_belongs_to_its_new_window() {
    let Some(shell) = shell() else { return };
    let rig = Rig::new();
    let pid = rig.start("t", "main", &shell) as i32;
    // macOS rebuilt the page in a window that now reports another label.
    assert!(rig.open("t", "window-2", &shell).reattached);
    rig.host.kill_window("main");
    assert!(!gone(pid));
    assert!(rig.host.get("t").is_some());
    rig.host.kill_window("window-2");
    assert!(rig.host.get("t").is_none());
    wait_until(|| gone(pid), "the shell outlived its window");
}

#[test]
fn closing_a_window_stops_only_its_terminals() {
    let Some(shell) = shell() else { return };
    let rig = Rig::new();
    let closing = rig.start("a", "main", &shell) as i32;
    let closing_job = rig.start_job("a", "sleep 1000");
    let staying = rig.start("b", "window-2", &shell) as i32;
    let staying_job = rig.start_job("b", "sleep 1000");

    rig.host.kill_window("main");
    // `kill_window` waits for the shells; its jobs get the hangup.
    assert!(gone(closing), "the closed window's shell is still running");
    wait_until(
        || group_gone(closing_job),
        "the closed window's job is still running",
    );
    assert!(rig.host.get("a").is_none());

    assert!(!gone(staying));
    assert!(!group_gone(staying_job));
    let from = rig.out.len("b");
    rig.write("b", b"\x03");
    rig.out.wait_for("b", from, PROMPT_READY);
    let out = rig.run("b", "echo still-$((3*3))");
    assert!(out.contains("still-9"), "{out:?}");
}

#[test]
fn closing_a_window_then_quitting_stops_everything_before_exit() {
    let Some(shell) = shell() else { return };
    let rig = Rig::new();
    let closed = rig.start("a", "window-2", &shell) as i32;
    let closed_job = rig.start_job("a", "sleep 1000");
    let main = rig.start("b", "main", &shell) as i32;
    let main_job = rig.start_job("b", "sleep 1000");

    // What `RunEvent::WindowEvent(Destroyed)` does for a non-last window...
    let host = rig.host.clone();
    rig.host
        .track_reaper(thread::spawn(move || host.kill_window("window-2")));
    // ...immediately followed by Quit's `reap_harness_children`.
    rig.host.kill_all();
    rig.host.join_reapers();

    // Both paths have finished their SIGKILL escalation by now; nothing is
    // left for the exiting process to orphan.
    assert!(gone(closed), "the closed window's shell outlived quit");
    assert!(gone(main), "the main window's shell outlived quit");
    wait_until(
        || group_gone(closed_job) && group_gone(main_job),
        "a job outlived its shell",
    );
    assert!(rig.host.get("a").is_none() && rig.host.get("b").is_none());
}
