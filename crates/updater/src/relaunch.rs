//! Start the app again after an update, replacing `relaunch()` from
//! tauri-plugin-process.
//!
//! Tauri started the new binary and exited the old process at once. Here a
//! small detached helper waits until this process is gone and then starts the
//! app, so the caller can quit normally (GPUI's `cx.quit()`) and its shutdown
//! hooks still flush the store and settings before the new copy opens them.
//! On macOS the helper uses `open` on the bundle, so a renamed executable is
//! found from the new `Info.plist`, as Tauri's restart read it.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::{current_binary, extract_path_from_executable};

/// Schedules the relaunch and returns. Quit the app right after.
pub fn relaunch() -> io::Result<()> {
    let binary = current_binary()?;
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let mut command = relaunch_command(std::process::id(), &binary, &args);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        // Its own process group, so it outlives ours.
        command.process_group(0);
    }
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    Ok(())
}

/// What to start: the `.app` around `binary` on macOS, else `binary`.
fn launch_target(binary: &Path) -> (PathBuf, bool) {
    if cfg!(target_os = "macos")
        && let Ok(bundle) = extract_path_from_executable(binary)
        && bundle.extension().and_then(|ext| ext.to_str()) == Some("app")
    {
        return (bundle, true);
    }
    (binary.to_path_buf(), false)
}

/// The helper command for process `pid`.
pub(crate) fn relaunch_command(pid: u32, binary: &Path, args: &[OsString]) -> Command {
    let (target, is_bundle) = launch_target(binary);
    if cfg!(windows) {
        // The NSIS installer restarts the app after an update, so this only
        // runs for a plain restart. Start the binary directly, as Tauri did.
        let mut command = Command::new(target);
        command.args(args);
        return command;
    }

    // `$0` is the pid to wait for, the rest is the command to start.
    let script = r#"while kill -0 "$0" 2>/dev/null; do sleep 0.1; done; exec "$@""#;
    let mut command = Command::new("/bin/sh");
    command.arg("-c").arg(script).arg(pid.to_string());
    if is_bundle {
        command.arg("/usr/bin/open").arg(target);
        if !args.is_empty() {
            command.arg("--args").args(args);
        }
    } else {
        command.arg(target).args(args);
    }
    command
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn argv(command: &Command) -> Vec<String> {
        command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn reopens_the_bundle_on_macos() {
        let command = relaunch_command(
            42,
            Path::new("/Applications/MonoCode.app/Contents/MacOS/MonoCode"),
            &["--flag".into()],
        );
        assert_eq!(command.get_program(), "/bin/sh");
        let args = argv(&command);
        assert_eq!(
            &args[2..],
            [
                "42",
                "/usr/bin/open",
                "/Applications/MonoCode.app",
                "--args",
                "--flag"
            ]
        );
    }

    #[test]
    fn restarts_a_loose_binary_with_its_arguments() {
        let command = relaunch_command(7, Path::new("/opt/monocode/monocode"), &["a b".into()]);
        let args = argv(&command);
        assert_eq!(&args[2..], ["7", "/opt/monocode/monocode", "a b"]);
    }

    #[test]
    fn the_helper_waits_for_the_pid_and_then_runs_the_command() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("ran");
        // A pid that is already gone, so the helper runs at once.
        let mut child = Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        let mut command = relaunch_command(
            pid,
            Path::new("/usr/bin/touch"),
            &[marker.clone().into_os_string()],
        );
        assert!(command.status().unwrap().success());
        assert!(marker.exists());
    }
}
