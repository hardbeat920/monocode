//! Keep agent descendants and optional indexing below the interactive UI.
use std::process::Command;
use std::sync::{Arc, OnceLock};
use tokio::sync::Semaphore;

/// Applied in the child before exec, so compiler/tool descendants inherit it.
/// Best effort: an OS refusing a scheduling hint must not prevent a tool launch.
pub(crate) fn configure_child(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Only system calls here: the post-fork child must not allocate or lock.
        unsafe {
            command.pre_exec(|| {
                let priority = libc::getpriority(libc::PRIO_PROCESS, 0).max(10);
                libc::setpriority(libc::PRIO_PROCESS, 0, priority);
                #[cfg(target_os = "macos")]
                setiopolicy_np(IOPOL_TYPE_DISK, IOPOL_SCOPE_PROCESS, IOPOL_THROTTLE);
                Ok(())
            });
        }
    }
    let _ = command;
}

/// Acquire before entering the blocking pool, so queued reads don't occupy
/// hundreds of threads. User mutations keep their existing independent path.
pub(crate) async fn read<F, R>(work: F) -> Result<R, String>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    static LIMIT: OnceLock<Arc<Semaphore>> = OnceLock::new();
    let permit = LIMIT
        .get_or_init(|| Arc::new(Semaphore::new(4)))
        .clone()
        .acquire_owned()
        .await
        .map_err(|error| error.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        #[cfg(target_os = "macos")]
        let _priority = UtilityPriority::enter();
        work()
    })
    .await
    .map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
const IOPOL_TYPE_DISK: i32 = 0;
#[cfg(target_os = "macos")]
const IOPOL_SCOPE_PROCESS: i32 = 0;
#[cfg(target_os = "macos")]
const IOPOL_SCOPE_THREAD: i32 = 1;
#[cfg(target_os = "macos")]
const IOPOL_THROTTLE: i32 = 3;

#[cfg(target_os = "macos")]
const QOS_CLASS_DEFAULT: u32 = 0x15;
#[cfg(target_os = "macos")]
const QOS_CLASS_UTILITY: u32 = 0x11;

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn pthread_get_qos_class_np(thread: libc::pthread_t, qos: *mut u32, relative: *mut i32) -> i32;
    fn pthread_set_qos_class_self_np(qos: u32, relative: i32) -> i32;
    fn setiopolicy_np(kind: i32, scope: i32, policy: i32) -> i32;
    fn getiopolicy_np(kind: i32, scope: i32) -> i32;
}

#[cfg(target_os = "macos")]
struct UtilityPriority {
    qos: u32,
    relative: i32,
    io: i32,
}

#[cfg(target_os = "macos")]
impl UtilityPriority {
    fn enter() -> Self {
        unsafe {
            let mut previous = Self {
                qos: QOS_CLASS_DEFAULT,
                relative: 0,
                io: getiopolicy_np(IOPOL_TYPE_DISK, IOPOL_SCOPE_THREAD),
            };
            pthread_get_qos_class_np(
                libc::pthread_self(),
                &mut previous.qos,
                &mut previous.relative,
            );
            pthread_set_qos_class_self_np(QOS_CLASS_UTILITY, 0);
            setiopolicy_np(IOPOL_TYPE_DISK, IOPOL_SCOPE_THREAD, IOPOL_THROTTLE);
            previous
        }
    }
}

#[cfg(target_os = "macos")]
impl Drop for UtilityPriority {
    fn drop(&mut self) {
        unsafe {
            let qos = if self.qos == 0 {
                QOS_CLASS_DEFAULT
            } else {
                self.qos
            };
            pthread_set_qos_class_self_np(qos, self.relative);
            if self.io >= 0 {
                setiopolicy_np(IOPOL_TYPE_DISK, IOPOL_SCOPE_THREAD, self.io);
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn scheduling_policy_reaches_child_and_descendant() {
        let mut child = Command::new(std::env::current_exe().unwrap());
        child
            .args([
                "--exact",
                "background_work::tests::child_policy_probe",
                "--nocapture",
            ])
            .env("MONOCODE_PRIORITY_PROBE", "1");
        configure_child(&mut child);
        let output = child.output().unwrap();
        assert!(
            output.status.success(),
            "{} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn child_policy_probe() {
        if std::env::var("MONOCODE_PRIORITY_PROBE").as_deref() != Ok("1") {
            return;
        }
        assert!(unsafe { libc::getpriority(libc::PRIO_PROCESS, 0) } >= 10);
        #[cfg(target_os = "macos")]
        assert_eq!(
            unsafe { getiopolicy_np(IOPOL_TYPE_DISK, IOPOL_SCOPE_PROCESS) },
            IOPOL_THROTTLE
        );
        let output = Command::new("/bin/sh")
            .args(["-c", "ps -o ni= -p $$"])
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&output.stdout)
                .trim()
                .parse::<i32>()
                .unwrap()
                >= 10
        );
    }
}
