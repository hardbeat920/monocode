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
    active()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(pid, agent);
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
    if shutting_down() {
        cancel.store(true, Ordering::SeqCst);
    }
    turns()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(id, cancel);
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
    let pids: Vec<_> = active()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .keys()
        .copied()
        .collect();
    for pid in pids {
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;
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
