//! Child policy extracted from src-tauri/src/background_work.rs.
use std::process::Command;

/// Apply scheduling hints before exec so descendants inherit them.
/// Best effort: a refused hint must not prevent a provider from launching.
pub fn configure_child(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Only system calls here: the post-fork child must not allocate or lock.
        unsafe {
            command.pre_exec(|| {
                // A GUI executor's worker may block signals. exec preserves
                // that mask; Tokio-based provider children then never receive
                // SIGCHLD and their command sessions never finish. Give the
                // provider the ordinary process mask before it starts.
                let mut signals = std::mem::zeroed();
                libc::sigemptyset(&mut signals);
                if libc::sigprocmask(libc::SIG_SETMASK, &signals, std::ptr::null_mut()) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                let priority = libc::getpriority(libc::PRIO_PROCESS, 0).max(10);
                libc::setpriority(libc::PRIO_PROCESS, 0, priority);
                #[cfg(target_os = "macos")]
                setiopolicy_np(0, 0, 3); // DISK, PROCESS, THROTTLE
                Ok(())
            });
        }
    }
    // A GUI-subsystem parent gives every console child (git, gh) its own
    // visible console window unless asked not to.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let _ = command;
}

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn setiopolicy_np(kind: i32, scope: i32, policy: i32) -> i32;
}

/// New native provider processes use a separate group for cancellation.
pub fn isolate(command: &mut Command) {
    configure_child(command);
    command.env("MONOCODE_HARNESS_PARENT", std::process::id().to_string());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW | 0x0000_0200);
    }
}

/// Called only by the provider worker. Always reap the immediate child.
pub fn terminate(child: &mut std::process::Child) {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    #[cfg(windows)]
    {
        let mut kill = Command::new("taskkill");
        kill.args(["/PID", &child.id().to_string(), "/T", "/F"]);
        use std::os::windows::process::CommandExt;
        kill.creation_flags(CREATE_NO_WINDOW);
        let _ = kill.status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// How long a provider whose stdin was closed may take to exit by itself.
#[cfg(unix)]
const EXIT_GRACE: std::time::Duration = std::time::Duration::from_millis(1500);
/// How long the group leader may take to shut down after SIGTERM.
#[cfg(unix)]
const TERM_GRACE: std::time::Duration = std::time::Duration::from_secs(2);

/// Stop a provider child without cutting off a credential write, on a
/// background thread. Close its stdin before calling this.
///
/// Claude Code rotates its OAuth refresh token on every refresh and saves the
/// new one through a `security -i` child. Killing the whole group in that
/// window loses the new token and leaves Keychain holding a spent one, which
/// signs the account out. Close stdin first, then send SIGTERM to the leader
/// only (Claude shuts down cleanly on it), and SIGKILL the group last.
pub fn retire(mut child: std::process::Child) {
    drop(child.stdin.take());
    #[cfg(unix)]
    {
        let pid = child.id();
        // std never reaps a dropped Child; `retire_pid` waits for it below.
        drop(child);
        std::thread::spawn(move || retire_pid(pid, || {}));
    }
    #[cfg(not(unix))]
    std::thread::spawn(move || terminate(&mut child));
}

/// Blocking form of [`retire`] for a child whose `Child` value cannot be
/// moved. `after` runs once the process is gone (to join pipe readers).
#[cfg(unix)]
pub(crate) fn retire_pid(pid: u32, after: impl FnOnce()) {
    let pid = pid as libc::pid_t;
    let exited = |within: std::time::Duration| {
        let deadline = std::time::Instant::now() + within;
        loop {
            let mut status = 0;
            match unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) } {
                0 if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(20))
                }
                0 => return false,
                // Reaped now, or already reaped / not our child.
                _ => return true,
            }
        }
    };
    if !exited(EXIT_GRACE) {
        unsafe { libc::kill(pid, libc::SIGTERM) };
        if !exited(TERM_GRACE) {
            unsafe { libc::kill(-pid, libc::SIGKILL) };
            let mut status = 0;
            unsafe { libc::waitpid(pid, &mut status, 0) };
        }
    }
    // The leader has finished its own writes. Clear any helpers it left behind.
    unsafe { libc::kill(-pid, libc::SIGKILL) };
    after();
}

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
/// A provider child that belongs to a session, for the running-work list.
#[derive(Clone, Debug)]
pub struct Agent {
    pub session: i64,
    pub pid: u32,
    /// Birth stamp at spawn; a reused PID no longer matches it.
    pub identity: Option<String>,
}
fn active() -> &'static Mutex<HashMap<u32, Option<Agent>>> {
    static ACTIVE: OnceLock<Mutex<HashMap<u32, Option<Agent>>>> = OnceLock::new();
    ACTIVE.get_or_init(|| Mutex::new(HashMap::new()))
}
thread_local! {
    static OWNER: Cell<Option<i64>> = const { Cell::new(None) };
}
/// Children spawned by `run` on this thread are attributed to `session`.
pub fn owned_by<T>(session: i64, run: impl FnOnce() -> T) -> T {
    struct Restore(Option<i64>);
    impl Drop for Restore {
        fn drop(&mut self) {
            OWNER.set(self.0);
        }
    }
    let _restore = Restore(OWNER.replace(Some(session)));
    run()
}
/// Live provider children that belong to a session.
pub fn agents() -> Vec<Agent> {
    active()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .values()
        .flatten()
        .cloned()
        .collect()
}
pub(crate) fn owner() -> Option<i64> {
    OWNER.get()
}
pub(crate) fn mark_idle(pid: u32, idle: bool) {
    let mut processes = active().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(slot) = processes.get_mut(&pid) {
        *slot = if idle {
            None
        } else {
            OWNER.get().map(|session| Agent {
                session,
                pid,
                identity: crate::resources::identity(pid),
            })
        };
    }
}
pub(crate) struct Registration(u32);
pub(crate) fn register(pid: u32) -> Registration {
    let agent = OWNER.get().map(|session| Agent {
        session,
        pid,
        identity: crate::resources::identity(pid),
    });
    let late = {
        let mut processes = active().lock().unwrap_or_else(|e| e.into_inner());
        processes.insert(pid, agent);
        CHILDREN_KILLED.load(Ordering::SeqCst)
    };
    // It started after the quit's last look, so nothing else will stop it. It
    // only just started, so it has no credential write to protect.
    if late {
        kill_group(pid);
    }
    Registration(pid)
}
impl Drop for Registration {
    fn drop(&mut self) {
        active()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.0);
    }
}
/// Explicit native-app quit hook. Run off the UI thread and wait briefly for the
/// workers to reap their children, rather than leaving isolated provider groups.
///
/// Leaders get SIGTERM first so a CLI that is saving a rotated OAuth token
/// (Claude Code) can finish; only what is still running after that is killed.
pub fn shutdown_all() {
    QUITTING.store(true, Ordering::SeqCst);
    // Cancel first so providers can abort their remote turn (OpenCode's managed
    // service outlives MonoCode) and workers can settle their rows.
    cancel_turns();
    // Most providers stop well within this; a slower one is killed below.
    drain_turns(std::time::Duration::from_millis(750));
    kill_children();
    drain_turns(std::time::Duration::from_secs(1));
}

/// The note on a tool a quit cut short. It is an interruption, not a failure.
pub const INTERRUPTED_BY_QUIT: &str = "Interrupted when MonoCode quit.";

static QUITTING: AtomicBool = AtomicBool::new(false);
/// Set under the `active` lock when the quit takes its final list of children.
static CHILDREN_KILLED: AtomicBool = AtomicBool::new(false);
/// True once the app has begun quitting. A provider that fails now was killed
/// by the quit, so its turn is interrupted rather than failed or finished.
pub fn shutting_down() -> bool {
    QUITTING.load(Ordering::SeqCst)
}
fn turns() -> &'static Mutex<HashMap<u64, Arc<AtomicBool>>> {
    static TURNS: OnceLock<Mutex<HashMap<u64, Arc<AtomicBool>>>> = OnceLock::new();
    TURNS.get_or_init(Default::default)
}
/// A running turn's cancel flag, raised when the app quits.
pub struct TurnRegistration(u64);
pub fn register_turn(cancel: Arc<AtomicBool>) -> TurnRegistration {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    // Check under the lock: `cancel_turns` either sees this entry, or held the
    // lock first and QUITTING is already visible here.
    let mut registry = turns().lock().unwrap_or_else(|e| e.into_inner());
    if shutting_down() {
        cancel.store(true, Ordering::SeqCst);
    }
    registry.insert(id, cancel);
    TurnRegistration(id)
}
impl Drop for TurnRegistration {
    fn drop(&mut self) {
        turns()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.0);
    }
}
fn cancel_turns() {
    for cancel in turns().lock().unwrap_or_else(|e| e.into_inner()).values() {
        cancel.store(true, Ordering::SeqCst);
    }
}
fn drain_turns(limit: std::time::Duration) {
    let deadline = std::time::Instant::now() + limit;
    while !turns().lock().unwrap_or_else(|e| e.into_inner()).is_empty()
        && std::time::Instant::now() < deadline
    {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn kill_children() {
    let registered = || -> Vec<u32> {
        active()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .copied()
            .collect()
    };
    #[cfg(unix)]
    {
        for pid in registered() {
            unsafe { libc::kill(pid as i32, libc::SIGTERM) };
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !registered().is_empty() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
    // Past this point `register` stops a late child itself, since it can't be in
    // this list. The flag and the list share the lock, so no child misses both.
    let pids: Vec<_> = {
        let processes = active().lock().unwrap_or_else(|e| e.into_inner());
        CHILDREN_KILLED.store(true, Ordering::SeqCst);
        processes.keys().copied().collect()
    };
    for pid in pids {
        kill_group(pid);
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    while !active()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_empty()
        && std::time::Instant::now() < deadline
    {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn kill_group(pid: u32) {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
    }
    #[cfg(windows)]
    {
        let mut c = Command::new("taskkill");
        c.args(["/PID", &pid.to_string(), "/T", "/F"]);
        use std::os::windows::process::CommandExt;
        c.creation_flags(CREATE_NO_WINDOW);
        let _ = c.status();
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    fn spawn(script: &str) -> std::process::Child {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", script]).stdin(Stdio::piped());
        isolate(&mut command);
        command.spawn().unwrap()
    }
    #[test]
    // `retire_pid` reaps the child with waitpid.
    #[allow(clippy::zombie_processes)]
    fn retire_lets_a_child_finish_on_stdin_eof_before_any_signal() {
        let dir = std::env::temp_dir().join(format!("monocode-retire-{}", std::process::id()));
        let _ = std::fs::remove_file(&dir);
        // Exits 0 on EOF after a short write; a signal would skip the write.
        let mut child = spawn(&format!(
            "trap '' TERM; cat >/dev/null; sleep 0.2; echo saved > '{}'",
            dir.display()
        ));
        let pid = child.id();
        drop(child.stdin.take());
        let started = Instant::now();
        retire_pid(pid, || {});
        assert!(started.elapsed() < EXIT_GRACE);
        assert_eq!(std::fs::read_to_string(&dir).unwrap().trim(), "saved");
        let _ = std::fs::remove_file(&dir);
    }
    #[test]
    // `retire_pid` reaps the child with waitpid.
    #[allow(clippy::zombie_processes)]
    fn retire_escalates_when_a_child_ignores_stdin_and_sigterm() {
        let mut child = spawn("trap '' TERM; while :; do sleep 0.05; done");
        let pid = child.id();
        drop(child.stdin.take());
        let started = Instant::now();
        retire_pid(pid, || {});
        assert!(started.elapsed() >= EXIT_GRACE + TERM_GRACE - Duration::from_millis(100));
        // The group is sent SIGKILL as `retire_pid` returns; its other members
        // (the loop's `sleep`) can take a moment to exit and be reaped, longer
        // on a loaded CI runner.
        let deadline = Instant::now() + Duration::from_secs(2);
        while unsafe { libc::kill(-(pid as i32), 0) } == 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(unsafe { libc::kill(-(pid as i32), 0) }, -1);
    }
    #[test]
    fn quitting_cancels_registered_turns_and_waits_for_them() {
        let cancel = Arc::new(AtomicBool::new(false));
        let registration = register_turn(cancel.clone());
        cancel_turns();
        assert!(cancel.load(Ordering::SeqCst));
        let worker = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(50));
            drop(registration);
        });
        let started = std::time::Instant::now();
        drain_turns(std::time::Duration::from_secs(5));
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        worker.join().unwrap();
    }
    /// Quitting is process-wide and one-way, so a test that quits runs alone in
    /// a fresh copy of this binary. True in that copy; the parent checks it.
    fn runs_alone(test: &str) -> bool {
        const ALONE: &str = "MONOCODE_QUIT_TEST_ALONE";
        if std::env::var_os(ALONE).is_some() {
            return true;
        }
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test, "--nocapture"])
            .env(ALONE, "1")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success() && stdout.contains("1 passed"),
            "{stdout}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        false
    }
    #[test]
    fn a_turn_registering_while_the_quit_cancels_turns_is_cancelled() {
        if !runs_alone(
            "process::tests::a_turn_registering_while_the_quit_cancels_turns_is_cancelled",
        ) {
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        // Holding the registry stops the worker at its lock, the point where a
        // check made before locking has already seen no quit.
        let registry = turns().lock().unwrap();
        let worker = std::thread::spawn({
            let cancel = cancel.clone();
            move || register_turn(cancel)
        });
        std::thread::sleep(Duration::from_millis(100));
        // The quit's cancel pass runs now and cannot see this turn.
        QUITTING.store(true, Ordering::SeqCst);
        assert!(registry.is_empty());
        drop(registry);
        let _registration = worker.join().unwrap();
        assert!(cancel.load(Ordering::SeqCst));
    }
    #[test]
    fn a_child_registering_after_the_quit_listed_children_is_killed() {
        if !runs_alone(
            "process::tests::a_child_registering_after_the_quit_listed_children_is_killed",
        ) {
            return;
        }
        let mut child = spawn("sleep 5");
        shutdown_all();
        let _registration = register(child.id());
        let deadline = Instant::now() + Duration::from_secs(2);
        while child.try_wait().unwrap().is_none() {
            assert!(Instant::now() < deadline, "the late child kept running");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    #[test]
    fn provider_child_does_not_inherit_executor_signal_mask() {
        const CHILD: &str = "MONOCODE_SIGNAL_REGRESSION_CHILD";
        unsafe {
            let mut mask = std::mem::zeroed();
            libc::pthread_sigmask(libc::SIG_SETMASK, std::ptr::null(), &mut mask);
            if std::env::var_os(CHILD).is_some() {
                assert_eq!(libc::sigismember(&mask, libc::SIGCHLD), 0);
                return;
            }
            let mut blocked = std::mem::zeroed();
            libc::sigemptyset(&mut blocked);
            libc::sigaddset(&mut blocked, libc::SIGCHLD);
            assert_eq!(
                libc::pthread_sigmask(libc::SIG_BLOCK, &blocked, std::ptr::null_mut()),
                0
            );
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--exact",
                    "process::tests::provider_child_does_not_inherit_executor_signal_mask",
                    "--nocapture",
                ])
                .env(CHILD, "1");
            isolate(&mut command);
            let result = command.output();
            libc::pthread_sigmask(libc::SIG_SETMASK, &mask, std::ptr::null_mut());
            let output = result.unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stdout)
            );
        }
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    #[test]
    fn background_children_keep_piped_output_and_exit_status() {
        let mut command = Command::new("cmd.exe");
        command.args(["/D", "/C", "(echo stdout)&(echo stderr 1>&2)&exit /b 7"]);
        configure_child(&mut command);

        let output = command.output().expect("background command should run");
        assert_eq!(output.status.code(), Some(7));
        assert!(String::from_utf8_lossy(&output.stdout).contains("stdout"));
        assert!(String::from_utf8_lossy(&output.stderr).contains("stderr"));
    }
}
