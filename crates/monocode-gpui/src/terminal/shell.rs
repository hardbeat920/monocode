//! Platform shell selection, independent of the terminal renderer.

#[cfg(windows)]
pub(super) fn default_shell() -> (String, Vec<String>) {
    windows_shell(std::env::var("COMSPEC").ok())
}

#[cfg(windows)]
fn windows_shell(comspec: Option<String>) -> (String, Vec<String>) {
    // SHELL can be inherited from MSYS/Git Bash as a Unix path. Windows must
    // use its native command interpreter instead of falling back to /bin/bash.
    let shell = comspec
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "cmd.exe".into());
    (shell, Vec::new())
}

#[cfg(not(windows))]
pub(super) fn default_shell() -> (String, Vec<String>) {
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            if cfg!(target_os = "macos") {
                "/bin/zsh".into()
            } else {
                "/bin/bash".into()
            }
        });
    let stem = std::path::Path::new(&shell)
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let args = match stem {
        "zsh" | "bash" | "sh" | "fish" => vec!["-l".to_owned()],
        _ => Vec::new(),
    };
    (shell, args)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn windows_uses_comspec_without_unix_login_arguments() {
        let shell = r"C:\Windows\System32\cmd.exe";
        assert_eq!(windows_shell(Some(shell.into())), (shell.into(), vec![]));
    }

    #[test]
    fn windows_without_comspec_uses_cmd() {
        for value in [None, Some(String::new()), Some(" ".into())] {
            assert_eq!(windows_shell(value), ("cmd.exe".into(), vec![]));
        }
    }

    #[test]
    fn windows_default_shell_runs_a_command() {
        use std::os::windows::process::CommandExt;
        let (shell, args) = default_shell();
        let output = std::process::Command::new(shell)
            .args(args)
            .args(["/D", "/C", "echo monocode-terminal-ok"])
            .creation_flags(0x08000000) // CREATE_NO_WINDOW
            .output()
            .expect("the selected Windows shell must start");
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("monocode-terminal-ok"));
    }
}
