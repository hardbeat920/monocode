//! Platform shell selection, independent of the terminal renderer.

#[cfg(windows)]
pub(super) fn default_shell() -> (String, Vec<String>) {
    windows_shell(
        std::env::var("SHELL").ok().as_deref(),
        std::env::var("COMSPEC").ok().as_deref(),
        resolve_windows_shell,
    )
}

#[cfg(windows)]
fn windows_shell(
    preferred: Option<&str>,
    comspec: Option<&str>,
    resolve: impl Fn(&str) -> Option<String>,
) -> (String, Vec<String>) {
    // Honor SHELL only if it resolves to a native executable. Git/MSYS can
    // otherwise supply a Unix path such as /bin/bash that Windows cannot run.
    let shell = preferred
        .and_then(&resolve)
        .or_else(|| resolve("pwsh.exe"))
        .or_else(|| resolve("powershell.exe"))
        .or_else(|| comspec.and_then(&resolve))
        .or_else(|| resolve("cmd.exe"))
        .unwrap_or_else(|| "cmd.exe".into());
    let stem = std::path::Path::new(&shell)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let args = match stem.as_str() {
        "pwsh" | "powershell" => vec!["-NoLogo".into()],
        "bash" | "sh" | "zsh" | "fish" => vec!["-l".into()],
        _ => Vec::new(),
    };
    (shell, args)
}

#[cfg(windows)]
fn resolve_windows_shell(program: &str) -> Option<String> {
    use std::path::PathBuf;

    let program = program.trim().trim_matches('"');
    if program.is_empty() {
        return None;
    }
    let mut executable = PathBuf::from(program);
    if executable.extension().is_none() {
        executable.set_extension("exe");
    }
    if !executable.extension()?.eq_ignore_ascii_case("exe") {
        return None;
    }
    let mut candidates = Vec::new();
    if executable.is_absolute() {
        candidates.push(executable);
    } else if executable.components().count() == 1 {
        if let Some(path) = std::env::var_os("PATH") {
            candidates.extend(std::env::split_paths(&path).map(|dir| dir.join(&executable)));
        }
        // GUI launches can inherit a PATH that lacks PowerShell's install dir.
        match program.to_ascii_lowercase().as_str() {
            "pwsh" | "pwsh.exe" => {
                if let Some(root) = std::env::var_os("ProgramFiles") {
                    candidates.push(PathBuf::from(root).join("PowerShell/7/pwsh.exe"));
                }
            }
            "powershell" | "powershell.exe" | "cmd" | "cmd.exe" => {
                if let Some(root) = std::env::var_os("SystemRoot") {
                    let system = PathBuf::from(root).join("System32");
                    candidates.push(if program.to_ascii_lowercase().starts_with("powershell") {
                        system.join("WindowsPowerShell/v1.0/powershell.exe")
                    } else {
                        system.join("cmd.exe")
                    });
                }
            }
            _ => {}
        }
    }
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .map(|path| path.to_string_lossy().into_owned())
}

pub(super) fn pty_program(program: &str) -> String {
    // Alacritty escapes arguments on Windows, but inserts the program verbatim
    // into CreateProcessW's command line. PowerShell 7 lives under Program Files.
    #[cfg(windows)]
    return format!("\"{program}\"");
    #[cfg(not(windows))]
    program.to_owned()
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

    fn select(preferred: Option<&str>, available: &[&str]) -> (String, Vec<String>) {
        windows_shell(preferred, Some("custom-cmd.exe"), |program| {
            available.contains(&program).then(|| program.to_owned())
        })
    }

    #[test]
    fn windows_prefers_powershell_7_then_windows_powershell_then_cmd() {
        assert_eq!(
            select(None, &["pwsh.exe", "powershell.exe", "custom-cmd.exe"]),
            ("pwsh.exe".into(), vec!["-NoLogo".into()])
        );
        assert_eq!(
            select(None, &["powershell.exe", "custom-cmd.exe"]),
            ("powershell.exe".into(), vec!["-NoLogo".into()])
        );
        assert_eq!(
            select(None, &["custom-cmd.exe"]),
            ("custom-cmd.exe".into(), vec![])
        );
        assert_eq!(select(None, &["cmd.exe"]), ("cmd.exe".into(), vec![]));
        assert_eq!(select(None, &[]), ("cmd.exe".into(), vec![]));
    }

    #[test]
    fn windows_honors_a_resolvable_user_shell() {
        assert_eq!(
            select(Some("bash.exe"), &["bash.exe", "pwsh.exe"]),
            ("bash.exe".into(), vec!["-l".into()])
        );
        assert_eq!(
            select(Some("cmd.exe"), &["cmd.exe", "pwsh.exe"]),
            ("cmd.exe".into(), vec![])
        );
    }

    #[test]
    fn windows_ignores_missing_or_unix_shells() {
        for preferred in [
            None,
            Some(""),
            Some(" "),
            Some("/bin/bash"),
            Some("missing.exe"),
        ] {
            assert_eq!(
                select(preferred, &["pwsh.exe"]),
                ("pwsh.exe".into(), vec!["-NoLogo".into()])
            );
        }
        assert!(resolve_windows_shell("/bin/bash").is_none());
        assert!(resolve_windows_shell("missing.exe").is_none());
        assert!(resolve_windows_shell("script.cmd").is_none());
        assert!(resolve_windows_shell("").is_none());
    }

    #[test]
    fn windows_quotes_executable_paths_for_conpty() {
        assert_eq!(
            pty_program(r"C:\Program Files\PowerShell\7\pwsh.exe"),
            r#""C:\Program Files\PowerShell\7\pwsh.exe""#
        );
    }

    #[test]
    fn windows_default_shell_runs_a_command() {
        use std::os::windows::process::CommandExt;
        let (shell, args) = windows_shell(
            None,
            std::env::var("COMSPEC").ok().as_deref(),
            resolve_windows_shell,
        );
        let powershell = std::path::Path::new(&shell)
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .to_ascii_lowercase();
        let command_args = if matches!(powershell.as_str(), "pwsh" | "powershell") {
            vec![
                "-NoProfile",
                "-Command",
                "Write-Output monocode-terminal-ok",
            ]
        } else {
            vec!["/D", "/C", "echo monocode-terminal-ok"]
        };
        let output = std::process::Command::new(shell)
            .args(args)
            .args(command_args)
            .creation_flags(0x08000000) // CREATE_NO_WINDOW
            .output()
            .expect("the selected Windows shell must start");
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("monocode-terminal-ok"));
    }
}
