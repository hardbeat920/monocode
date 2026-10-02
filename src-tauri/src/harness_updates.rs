use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::Value;

use crate::harness::{exec_output, is_resolved_harness_binary};

const REGISTRY_URL: &str = "https://registry.npmjs.org";
const CURSOR_INSTALL_URL: &str = "https://cursor.com/install";
const USER_AGENT: &str = "MonoCode";
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

/// Where each harness publishes its latest release. Hermes updates to the
/// newest commit rather than a release, and Antigravity's ACP server reports
/// no version, so neither has anything to compare against.
#[derive(Debug, PartialEq)]
enum VersionFeed {
    Npm(&'static str),
    /// A plain-text file whose first line is the version.
    Text(&'static str),
    /// Cursor has no version file. Its install script downloads one pinned
    /// build, and that build is the latest.
    CursorInstallScript,
}

fn version_feed(provider: &str) -> Option<VersionFeed> {
    match provider {
        "claude" => Some(VersionFeed::Npm("@anthropic-ai/claude-code")),
        "codex" => Some(VersionFeed::Npm("@openai/codex")),
        "opencode" => Some(VersionFeed::Npm("opencode-ai")),
        "pi" => Some(VersionFeed::Npm("@earendil-works/pi-coding-agent")),
        "omp" => Some(VersionFeed::Npm("@oh-my-pi/pi-coding-agent")),
        "cursor" => Some(VersionFeed::CursorInstallScript),
        "grok" => Some(VersionFeed::Text("https://x.ai/cli/stable")),
        "fx" => Some(VersionFeed::Text("https://releases.fx.sh/latest.txt")),
        _ => None,
    }
}

/// Each CLI's own updater, which knows how it was installed (native, npm,
/// Homebrew) better than MonoCode could guess from the binary path.
fn update_args(provider: &str) -> Option<&'static [&'static str]> {
    match provider {
        "claude" => Some(&["update"]),
        "codex" => Some(&["update"]),
        "opencode" => Some(&["upgrade"]),
        "pi" => Some(&["update", "--self"]),
        "omp" => Some(&["update"]),
        "cursor" => Some(&["update"]),
        "grok" => Some(&["update"]),
        "fx" => Some(&["upgrade"]),
        _ => None,
    }
}

/// A download plus, for npm installs, a full dependency install.
const UPDATE_TIMEOUT: Duration = Duration::from_secs(300);

static LAUNCH_CHECK_CLAIMED: AtomicBool = AtomicBool::new(false);

/// True for the first caller per app process, so a window opened later in the
/// same run does not repeat the launch check.
#[tauri::command]
pub fn harness_update_check_claim() -> bool {
    !LAUNCH_CHECK_CLAIMED.swap(true, Ordering::SeqCst)
}

#[tauri::command]
pub async fn harness_latest_version(provider: String) -> Result<String, String> {
    let feed =
        version_feed(&provider).ok_or_else(|| format!("No update feed for harness: {provider}"))?;
    tauri::async_runtime::spawn_blocking(move || match feed {
        VersionFeed::Npm(package) => {
            let text = fetch_text(
                &format!("{REGISTRY_URL}/{package}/latest"),
                "application/json",
            )?;
            let body: Value = serde_json::from_str(&text)
                .map_err(|error| format!("npm registry returned invalid JSON: {error}"))?;
            latest_version(&body).ok_or_else(|| "npm registry returned no version".to_string())
        }
        VersionFeed::Text(url) => first_line_version(&fetch_text(url, "text/plain")?)
            .ok_or_else(|| format!("{url} returned no version")),
        VersionFeed::CursorInstallScript => {
            cursor_script_version(&fetch_text(CURSOR_INSTALL_URL, "text/plain")?)
                .ok_or_else(|| "Cursor install script names no build".to_string())
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

fn fetch_text(url: &str, accept: &str) -> Result<String, String> {
    ureq::AgentBuilder::new()
        .timeout(HTTP_TIMEOUT)
        .build()
        .get(url)
        .set("User-Agent", USER_AGENT)
        .set("Accept", accept)
        .call()
        .map_err(|error| format!("{url} request failed: {error}"))?
        .into_string()
        .map_err(|error| format!("{url} response unreadable: {error}"))
}

/// Runs the harness's self-update against the binary MonoCode resolved for
/// it. stdin is closed, so an updater that stops to ask fails instead of
/// hanging.
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
            return Ok(());
        }
        Err(update_failure(&output.stdout, &output.stderr))
    })
    .await
    .map_err(|e| e.to_string())?
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

fn first_line_version(text: &str) -> Option<String> {
    let version = text.lines().next()?.trim();
    version
        .chars()
        .any(|c| c.is_ascii_digit())
        .then(|| version.to_string())
}

/// Reads the build from the download URL the script uses, such as
/// `https://downloads.cursor.com/lab/2026.09.28-64d2043/${OS}/...`.
fn cursor_script_version(script: &str) -> Option<String> {
    const PREFIX: &str = "downloads.cursor.com/lab/";
    let start = script.find(PREFIX)? + PREFIX.len();
    let build = script[start..].split('/').next()?;
    let valid = build.starts_with(|c: char| c.is_ascii_digit())
        && build
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    valid.then(|| build.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_only_harnesses_with_a_release_feed() {
        assert_eq!(
            version_feed("claude"),
            Some(VersionFeed::Npm("@anthropic-ai/claude-code"))
        );
        assert_eq!(
            version_feed("omp"),
            Some(VersionFeed::Npm("@oh-my-pi/pi-coding-agent"))
        );
        assert_eq!(
            version_feed("cursor"),
            Some(VersionFeed::CursorInstallScript)
        );
        assert_eq!(
            version_feed("fx"),
            Some(VersionFeed::Text("https://releases.fx.sh/latest.txt"))
        );
        assert_eq!(version_feed("hermes"), None);
        assert_eq!(version_feed("antigravity"), None);
        assert_eq!(version_feed("../../evil"), None);
    }

    #[test]
    fn every_feed_has_an_updater() {
        for provider in [
            "claude", "codex", "opencode", "pi", "omp", "cursor", "grok", "fx",
        ] {
            assert!(version_feed(provider).is_some(), "{provider} feed");
            assert!(update_args(provider).is_some(), "{provider} updater");
        }
    }

    #[test]
    fn updates_only_through_each_cli_own_updater() {
        assert_eq!(update_args("pi"), Some(&["update", "--self"][..]));
        assert_eq!(update_args("opencode"), Some(&["upgrade"][..]));
        assert_eq!(update_args("fx"), Some(&["upgrade"][..]));
        assert_eq!(update_args("cursor"), Some(&["update"][..]));
        assert_eq!(update_args("hermes"), None);
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
    fn reads_version_from_registry_payload() {
        assert_eq!(
            latest_version(&json!({ "name": "opencode-ai", "version": "1.18.33" })),
            Some("1.18.33".to_string())
        );
        assert_eq!(latest_version(&json!({ "version": " " })), None);
        assert_eq!(latest_version(&json!({})), None);
    }

    #[test]
    fn reads_version_from_a_text_feed() {
        assert_eq!(first_line_version("1.0.46\n"), Some("1.0.46".to_string()));
        assert_eq!(first_line_version("v0.0.12"), Some("v0.0.12".to_string()));
        assert_eq!(first_line_version("<html>"), None);
        assert_eq!(first_line_version(""), None);
    }

    #[test]
    fn reads_build_from_cursor_install_script() {
        let script = "TEMP=x\nDOWNLOAD_URL=\"https://downloads.cursor.com/lab/2026.09.28-64d2043/${OS}/${ARCH}/agent-cli-package.tar.gz\"\n";
        assert_eq!(
            cursor_script_version(script),
            Some("2026.09.28-64d2043".to_string())
        );
        assert_eq!(cursor_script_version("echo no download here"), None);
        assert_eq!(
            cursor_script_version("downloads.cursor.com/lab/$(evil)/x"),
            None
        );
    }
}
