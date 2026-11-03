use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::Value;

use crate::harness::{
    exec_output, exec_output_with_env, is_resolved_harness_binary, resolve_gui_binary,
};

const REGISTRY_URL: &str = "https://registry.npmjs.org";
const USER_AGENT: &str = "MonoCode";
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

/// Only harnesses whose releases are published to npm. The rest ship through
/// their own installers with no public version feed to compare against.
fn npm_package(provider: &str) -> Option<&'static str> {
    match provider {
        "claude" => Some("@anthropic-ai/claude-code"),
        "codex" => Some("@openai/codex"),
        "opencode" => Some("opencode-ai"),
        "pi" => Some("@earendil-works/pi-coding-agent"),
        _ => None,
    }
}

/// Each CLI's own updater. Native and npm installs apply it themselves.
/// Homebrew, winget, mise, and apk installs print an upgrade command and
/// exit 0 without installing anything; `package_manager_handoff` runs that
/// command.
fn update_args(provider: &str) -> Option<&'static [&'static str]> {
    match provider {
        "claude" => Some(&["update"]),
        "codex" => Some(&["update"]),
        "opencode" => Some(&["upgrade"]),
        "pi" => Some(&["update", "--self"]),
        _ => None,
    }
}

/// A download plus, for npm installs, a full dependency install.
const UPDATE_TIMEOUT: Duration = Duration::from_secs(300);

/// Homebrew refreshes its formula index and then downloads the CLI. That is
/// slower than the CLI's own updater, which for these installs only prints
/// the command.
const PACKAGE_MANAGER_TIMEOUT: Duration = Duration::from_secs(600);

static LAUNCH_CHECK_CLAIMED: AtomicBool = AtomicBool::new(false);

/// True for the first caller per app process, so a window opened later in the
/// same run does not repeat the launch check.
#[tauri::command]
pub fn harness_update_check_claim() -> bool {
    !LAUNCH_CHECK_CLAIMED.swap(true, Ordering::SeqCst)
}

#[tauri::command]
pub async fn harness_latest_version(provider: String) -> Result<String, String> {
    let package =
        npm_package(&provider).ok_or_else(|| format!("No update feed for harness: {provider}"))?;
    tauri::async_runtime::spawn_blocking(move || {
        let agent = ureq::AgentBuilder::new().timeout(HTTP_TIMEOUT).build();
        let text = agent
            .get(&format!("{REGISTRY_URL}/{package}/latest"))
            .set("User-Agent", USER_AGENT)
            .set("Accept", "application/json")
            .call()
            .map_err(|error| format!("npm registry request failed: {error}"))?
            .into_string()
            .map_err(|error| format!("npm registry response unreadable: {error}"))?;
        let body: Value = serde_json::from_str(&text)
            .map_err(|error| format!("npm registry returned invalid JSON: {error}"))?;
        latest_version(&body).ok_or_else(|| "npm registry returned no version".to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Runs the harness's self-update against the binary MonoCode resolved for
/// it. stdin is closed, so an updater that stops to ask fails instead of
/// hanging. A package-manager install that only prints its upgrade command
/// is then upgraded with that command.
#[tauri::command]
pub async fn harness_update(
    command: String,
    binary_provider: String,
    binary_path: Option<String>,
) -> Result<(), String> {
    let args: Vec<String> = update_args(&binary_provider)
        .ok_or_else(|| format!("No updater for harness: {binary_provider}"))?
        .iter()
        .map(|arg| arg.to_string())
        .collect();
    tauri::async_runtime::spawn_blocking(move || {
        if !is_resolved_harness_binary(&command, Some(&binary_provider), binary_path.as_deref()) {
            return Err("harness_update: not a resolved harness CLI".to_string());
        }
        let output = exec_output(&command, &args, None, UPDATE_TIMEOUT)?;
        if output.status.success() {
            return apply_package_manager_handoff(&output.stdout, &output.stderr);
        }
        Err(update_failure(&output.stdout, &output.stderr))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// What an updater printed after exiting 0. `NotManaged` means it installed
/// the update itself. `Upgrade` is a package-manager command it told the
/// user to run. `Refused` is a command we will not execute.
#[derive(Debug)]
enum PackageManagerHandoff {
    NotManaged,
    Upgrade(PackageManagerCommand),
    Refused(String),
}

#[derive(Debug)]
struct PackageManagerCommand {
    program: String,
    args: Vec<String>,
}

impl PackageManagerCommand {
    fn display(&self) -> String {
        std::iter::once(self.program.as_str())
            .chain(self.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn apply_package_manager_handoff(stdout: &[u8], stderr: &[u8]) -> Result<(), String> {
    let command = match package_manager_handoff(stdout, stderr) {
        PackageManagerHandoff::NotManaged => return Ok(()),
        PackageManagerHandoff::Refused(reason) => return Err(reason),
        PackageManagerHandoff::Upgrade(command) => command,
    };
    let program = resolve_gui_binary(&command.program).ok_or_else(|| {
        format!(
            "Could not find {} on PATH. Run `{}` to finish the update.",
            command.program,
            command.display()
        )
    })?;
    let program = program.to_string_lossy().into_owned();
    let output = exec_output_with_env(
        &program,
        &command.args,
        None,
        PACKAGE_MANAGER_TIMEOUT,
        &[("NONINTERACTIVE", "1")],
    )?;
    if output.status.success() {
        return Ok(());
    }
    Err(update_failure(&output.stdout, &output.stderr))
}

/// Claude Code, and the other CLIs that follow it, exit 0 after printing
/// `To update, run:` for a Homebrew, winget, mise, or apk install. The
/// command is allowlisted and run as argv, never through a shell, so a tip
/// such as `brew uninstall && brew install` is not executed.
fn package_manager_handoff(stdout: &[u8], stderr: &[u8]) -> PackageManagerHandoff {
    let text = strip_ansi(&format!(
        "{}\n{}",
        String::from_utf8_lossy(stdout),
        String::from_utf8_lossy(stderr)
    ));
    let lines: Vec<&str> = text.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        let header = line.trim();
        if header != "To update, run:" && header != "To update manually, run:" {
            continue;
        }
        let Some(command_line) = lines[index + 1..]
            .iter()
            .map(|line| line.trim())
            .find(|line| !line.is_empty())
        else {
            return PackageManagerHandoff::Refused(
                "Updater said to run a package-manager command, but did not print one.".to_string(),
            );
        };
        return match parse_package_manager_command(command_line) {
            Some(command) => PackageManagerHandoff::Upgrade(command),
            None => PackageManagerHandoff::Refused(format!(
                "Updater asked for a package-manager command MonoCode will not run: {command_line}"
            )),
        };
    }
    PackageManagerHandoff::NotManaged
}

fn parse_package_manager_command(line: &str) -> Option<PackageManagerCommand> {
    if line.chars().any(|c| {
        c.is_control()
            || matches!(
                c,
                ';' | '|' | '&' | '`' | '$' | '<' | '>' | '(' | ')' | '{' | '}' | '"' | '\'' | '\\'
            )
    }) {
        return None;
    }
    let tokens: Vec<String> = line.split_whitespace().map(str::to_string).collect();
    let (program, args) = tokens.split_first()?;
    if !allowed_package_manager(program, args) {
        return None;
    }
    Some(PackageManagerCommand {
        program: program.clone(),
        args: args.to_vec(),
    })
}

fn allowed_package_manager(program: &str, args: &[String]) -> bool {
    match program {
        "brew" => match args {
            [action, package] if action == "upgrade" && is_package_token(package) => true,
            [action, flag, package]
                if action == "upgrade" && flag == "--cask" && is_package_token(package) =>
            {
                true
            }
            _ => false,
        },
        "winget" | "mise" | "apk" => {
            args.len() == 2 && args[0] == "upgrade" && is_package_token(&args[1])
        }
        _ => false,
    }
}

fn is_package_token(token: &str) -> bool {
    let mut chars = token.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphanumeric() => {}
        _ => return false,
    }
    (1..=80).contains(&token.len())
        && token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '@' | '+' | '.' | '_' | '-'))
}

fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }
        out.push(ch);
    }
    out
}

/// Updaters print their reason to either stream; the last line is the one
/// that says what went wrong.
fn update_failure(stdout: &[u8], stderr: &[u8]) -> String {
    [stderr, stdout]
        .iter()
        .filter_map(|bytes| {
            String::from_utf8_lossy(bytes)
                .lines()
                .map(str::trim)
                .rfind(|line| !line.is_empty())
                .map(str::to_string)
        })
        .next()
        .unwrap_or_else(|| "Update failed".to_string())
}

fn latest_version(body: &Value) -> Option<String> {
    let version = body.get("version")?.as_str()?.trim();
    (!version.is_empty()).then(|| version.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_only_npm_published_harnesses() {
        assert_eq!(npm_package("claude"), Some("@anthropic-ai/claude-code"));
        assert_eq!(npm_package("pi"), Some("@earendil-works/pi-coding-agent"));
        assert_eq!(npm_package("cursor"), None);
        assert_eq!(npm_package("../../evil"), None);
    }

    #[test]
    fn updates_only_through_each_cli_own_updater() {
        assert_eq!(update_args("pi"), Some(&["update", "--self"][..]));
        assert_eq!(update_args("opencode"), Some(&["upgrade"][..]));
        assert_eq!(update_args("cursor"), None);
    }

    #[test]
    fn reports_the_last_line_an_updater_printed() {
        assert_eq!(
            update_failure(
                b"checking\n",
                b"npm ERR! code EACCES\nnpm ERR! permission denied\n\n"
            ),
            "npm ERR! permission denied"
        );
        assert_eq!(update_failure(b"no write access\n", b""), "no write access");
        assert_eq!(update_failure(b"", b""), "Update failed");
    }

    #[test]
    fn runs_the_homebrew_upgrade_claude_prints_instead_of_installing() {
        let stdout = b"Current version: 2.1.284\nChecking for updates to latest version...\n\nClaude is managed by Homebrew.\nUpdate available: 2.1.284 \xe2\x86\x92 2.1.285\n\nTo update, run:\n  brew upgrade claude-code@latest\n";
        match package_manager_handoff(stdout, b"") {
            PackageManagerHandoff::Upgrade(command) => {
                assert_eq!(command.display(), "brew upgrade claude-code@latest");
            }
            other => panic!("expected the brew upgrade command, got {other:?}"),
        }
    }

    #[test]
    fn leaves_a_native_update_that_did_not_ask_for_a_package_manager() {
        let stdout = b"Successfully updated to version 2.1.285\n";
        assert!(matches!(
            package_manager_handoff(stdout, b""),
            PackageManagerHandoff::NotManaged
        ));
        let up_to_date = b"Claude is managed by Homebrew.\nClaude is up to date!\n\nTip: For more frequent updates, use the claude-code@latest cask:\n  brew uninstall --cask claude-code && brew install --cask claude-code@latest\n";
        assert!(matches!(
            package_manager_handoff(up_to_date, b""),
            PackageManagerHandoff::NotManaged
        ));
    }

    #[test]
    fn runs_winget_mise_and_apk_upgrade_commands() {
        for (stdout, expected) in [
            (
                "To update, run:\n  winget upgrade Anthropic.ClaudeCode\n",
                "winget upgrade Anthropic.ClaudeCode",
            ),
            (
                "To update manually, run:\n  mise upgrade claude\n",
                "mise upgrade claude",
            ),
            (
                "To update, run:\n  apk upgrade claude-code\n",
                "apk upgrade claude-code",
            ),
            (
                "To update, run:\n  brew upgrade --cask claude-code\n",
                "brew upgrade --cask claude-code",
            ),
        ] {
            match package_manager_handoff(stdout.as_bytes(), b"") {
                PackageManagerHandoff::Upgrade(command) => {
                    assert_eq!(command.display(), expected);
                }
                other => panic!("expected {expected}, got {other:?}"),
            }
        }
    }

    #[test]
    fn reads_an_upgrade_command_wrapped_in_ansi_bold() {
        let stdout = "To update, run:\n\u{1b}[1m  brew upgrade claude-code@latest\u{1b}[0m\n";
        match package_manager_handoff(stdout.as_bytes(), b"") {
            PackageManagerHandoff::Upgrade(command) => {
                assert_eq!(command.program, "brew");
                assert_eq!(
                    command.args,
                    vec!["upgrade".to_string(), "claude-code@latest".to_string()]
                );
            }
            other => panic!("expected the brew upgrade command, got {other:?}"),
        }
    }

    #[test]
    fn refuses_package_manager_commands_that_are_not_a_plain_upgrade() {
        for stdout in [
            "To update, run:\n  brew uninstall --cask claude-code && brew install --cask claude-code@latest\n",
            "To update, run:\n  brew upgrade claude-code; rm -rf /\n",
            "To update, run:\n  brew upgrade ../../evil\n",
            "To update, run:\n  sudo brew upgrade claude-code\n",
            "To update, run:\n  brew upgrade claude-code --force\n",
            "To update, run:\n\n",
        ] {
            assert!(
                matches!(
                    package_manager_handoff(stdout.as_bytes(), b""),
                    PackageManagerHandoff::Refused(_)
                ),
                "{stdout}"
            );
        }
    }

    #[test]
    fn reads_version_from_registry_payload() {
        assert_eq!(
            latest_version(&json!({ "name": "opencode-ai", "version": "1.18.33" })),
            Some("1.18.33".to_string())
        );
        assert_eq!(latest_version(&json!({ "version": " " })), None);
        assert_eq!(latest_version(&json!({})), None);
    }
}
