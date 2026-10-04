//! Port of host/process.ts: finds each provider's CLI on the host without
//! running anything that only shares its name.
//!
//! The host searches its own PATH and the usual install folders. Ambiguous
//! names (`agent`, `pi`, `fx`) must contain a provider marker before they
//! count, and the check reads the file instead of executing it.

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};

use monocode_remote::host::protocol::{RemoteProvider, provider_name};

/// `binaryNames`.
fn binary_names(provider: RemoteProvider) -> &'static [&'static str] {
    use monocode_core::HarnessId::*;
    match provider {
        Codex => &["codex"],
        Claude => &["claude"],
        Cursor => &["cursor-agent", "agent"],
        Grok => &["grok"],
        Opencode => &["opencode"],
        Pi => &["pi-coding-agent", "pi"],
        Omp => &["omp"],
        Fx => &["fx"],
        Hermes => &["hermes"],
        Droid => &["droid"],
        Antigravity => &["agy_acp_server.par"],
    }
}

/// `npmEntries`: the package entry point behind npm's Windows wrapper.
fn npm_entry(provider: &str) -> Option<&'static str> {
    match provider {
        "codex" => Some("node_modules/@openai/codex/bin/codex.js"),
        "claude" => Some("node_modules/@anthropic-ai/claude-code/cli.js"),
        _ => None,
    }
}

fn home() -> PathBuf {
    monocode_remote::host::server::home_dir()
}

/// `providerDirectories`: PATH first, then the usual install folders,
/// without duplicates.
pub fn provider_directories(provider: RemoteProvider, path: Option<&str>) -> Vec<PathBuf> {
    use monocode_core::HarnessId::*;
    let home = home();
    let extra: Vec<PathBuf> = match provider {
        Claude => vec![
            home.join(".claude").join("local"),
            home.join(".local").join("share").join("claude"),
        ],
        Grok => vec![home.join(".grok").join("bin")],
        Opencode => vec![home.join(".opencode").join("bin")],
        Fx => vec![home.join(".fx").join("bin")],
        Hermes => vec![
            home.join(".hermes")
                .join("hermes-agent")
                .join("venv")
                .join("bin"),
            home.join(".hermes")
                .join("hermes-agent")
                .join(".venv")
                .join("bin"),
        ],
        Droid => vec![home.join(".factory").join("bin"), home.join("bin")],
        Antigravity => vec![home.join(".local").join("share").join("agy-acp")],
        _ => Vec::new(),
    };
    let mut directories: Vec<PathBuf> = std::env::split_paths(path.unwrap_or_default()).collect();
    directories.extend([
        home.join(".local").join("bin"),
        home.join(".npm-global").join("bin"),
        home.join(".cargo").join("bin"),
        home.join("n").join("bin"),
        home.join(".bun").join("bin"),
    ]);
    directories.extend(extra);
    if cfg!(windows) {
        let roaming = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData").join("Roaming"));
        directories.push(roaming.join("npm"));
    } else {
        directories.extend(
            [
                "/opt/homebrew/bin",
                "/usr/local/bin",
                "/usr/bin",
                "/snap/bin",
            ]
            .map(PathBuf::from),
        );
    }
    let mut seen = HashSet::new();
    directories.retain(|directory| seen.insert(directory.clone()));
    directories
}

/// `matchesProvider`.
fn matches_provider(candidate: &Path, provider: RemoteProvider, name: &str) -> bool {
    use monocode_core::HarnessId::*;
    if provider == Cursor && name == "agent" {
        if let Ok(target) = std::fs::read_link(candidate)
            && target
                .to_string_lossy()
                .to_lowercase()
                .contains("cursor-agent")
        {
            return true;
        }
        return file_contains(candidate, &["cursor-agent"], 64 * 1024);
    }
    if provider == Pi && name == "pi" {
        return file_contains(
            candidate,
            &[
                "pi-coding-agent",
                "@earendil-works/pi",
                "@mariozechner/pi-coding-agent",
                "pi_coding_agent",
            ],
            64 * 1024,
        );
    }
    if provider == Fx {
        return file_contains(
            candidate,
            &["vercel-labs/fx", "fx_model", "createfxagent", "fx acp"],
            usize::MAX,
        );
    }
    true
}

/// `fileContains`: a case-insensitive scan of the first `max_bytes`, read
/// as Latin-1, with a 64-byte overlap between chunks.
fn file_contains(path: &Path, markers: &[&str], max_bytes: usize) -> bool {
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut read = 0usize;
    let mut carry = String::new();
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        let Ok(count) = file.read(&mut chunk) else {
            return false;
        };
        if count == 0 {
            return false;
        }
        let length = count.min(max_bytes - read);
        let mut text = carry.clone();
        text.extend(chunk[..length].iter().map(|byte| char::from(*byte)));
        let text = text.to_lowercase();
        if markers.iter().any(|marker| text.contains(marker)) {
            return true;
        }
        let chars: Vec<char> = text.chars().collect();
        carry = chars[chars.len().saturating_sub(64)..].iter().collect();
        read += length;
        if read >= max_bytes {
            return false;
        }
    }
}

#[cfg(unix)]
fn executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|meta| meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn executable(path: &Path) -> bool {
    path.exists()
}

/// `resolveProvider` with an explicit PATH, for tests.
pub fn resolve_provider_in(
    provider: RemoteProvider,
    path: Option<&str>,
) -> Result<PathBuf, String> {
    let windows = cfg!(windows);
    if windows && provider == monocode_core::HarnessId::Antigravity {
        return Err("Antigravity ACP is not available on Windows".into());
    }
    let extensions: &[&str] = if windows {
        &[".exe", ".cmd", ".bat", ".com"]
    } else {
        &[""]
    };
    for directory in provider_directories(provider, path) {
        let directory = directory.to_string_lossy();
        if directory.is_empty() {
            continue;
        }
        let directory = PathBuf::from(directory.trim_matches('"'));
        for name in binary_names(provider) {
            for extension in extensions {
                let candidate = directory.join(format!("{name}{extension}"));
                if !executable(&candidate) || !candidate.is_file() {
                    continue;
                }
                if provider_launch(&candidate.to_string_lossy(), &[], std::env::consts::OS).is_err()
                {
                    continue;
                }
                if !matches_provider(&candidate, provider, name) {
                    continue;
                }
                return Ok(candidate);
            }
        }
    }
    Err(format!(
        "{} is not installed on this host or is missing from its PATH. Install its native CLI or standard npm package.",
        provider_name(provider)
    ))
}

/// `resolveProvider`: the provider CLI on this host's PATH or in a usual
/// install folder.
pub fn resolve_provider(provider: RemoteProvider) -> Result<PathBuf, String> {
    let path = std::env::var("PATH").ok();
    resolve_provider_in(provider, path.as_deref())
}

/// What [`provider_launch`] runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub command: String,
    pub args: Vec<String>,
}

/// `providerLaunch`: npm's Windows `.cmd` wrappers cannot be spawned
/// directly, so run their known package entry point with Node, preserving
/// argv without a shell. Custom `.cmd` and `.bat` wrappers are refused
/// rather than interpreted as shell text. `platform` is `std::env::consts::OS`.
pub fn provider_launch(command: &str, args: &[String], platform: &str) -> Result<Launch, String> {
    let lower = command.to_lowercase();
    // TODO(port): the Node host ran script entry points with its own Node
    // (`process.execPath`). This host has no bundled Node, so it runs `node`
    // from PATH.
    let node = || "node".to_string();
    if platform == "windows" && (lower.ends_with(".cmd") || lower.ends_with(".bat")) {
        let path = Path::new(command);
        let provider = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let relative = npm_entry(&provider).ok_or("Unsupported Windows provider launcher")?;
        let entry = path.parent().unwrap_or(Path::new("")).join(relative);
        if !entry.is_file() {
            return Err("Missing npm provider entry point".into());
        }
        return Ok(Launch {
            command: node(),
            args: std::iter::once(entry.to_string_lossy().into_owned())
                .chain(args.iter().cloned())
                .collect(),
        });
    }
    if [".cjs", ".mjs", ".js"]
        .iter()
        .any(|extension| lower.ends_with(extension))
    {
        return Ok(Launch {
            command: node(),
            args: std::iter::once(command.to_string())
                .chain(args.iter().cloned())
                .collect(),
        });
    }
    Ok(Launch {
        command: command.into(),
        args: args.to_vec(),
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use monocode_core::HarnessId;

    fn script(path: &Path, body: &str) {
        std::fs::write(path, body).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    /// process.test.ts: "does not execute an unrelated ambiguous %s binary
    /// while resolving providers".
    #[cfg(unix)]
    #[test]
    fn does_not_execute_an_unrelated_ambiguous_binary_while_resolving_providers() {
        for provider in [HarnessId::Cursor, HarnessId::Pi, HarnessId::Fx] {
            let directory = tempfile::tempdir().unwrap();
            let name = if provider == HarnessId::Cursor {
                "agent"
            } else {
                provider_name(provider)
            };
            let candidate = directory.path().join(name);
            let sentinel = directory.path().join("executed");
            script(
                &candidate,
                &format!("#!/bin/sh\nprintf bad > '{}'\n", sentinel.to_string_lossy()),
            );
            let resolved = resolve_provider_in(provider, directory.path().to_str()).ok();
            assert_ne!(resolved.as_deref(), Some(candidate.as_path()));
            assert!(!sentinel.exists());
        }
    }

    /// process.test.ts: "recognizes a Cursor agent shim without executing it".
    #[cfg(unix)]
    #[test]
    fn recognizes_a_cursor_agent_shim_without_executing_it() {
        let directory = tempfile::tempdir().unwrap();
        let package = directory.path().join("cursor-agent-package");
        std::fs::create_dir(&package).unwrap();
        let target = package.join("cursor-agent");
        let candidate = directory.path().join("agent");
        let sentinel = directory.path().join("executed");
        script(
            &target,
            &format!("#!/bin/sh\nprintf bad > '{}'\n", sentinel.to_string_lossy()),
        );
        std::os::unix::fs::symlink(&target, &candidate).unwrap();
        assert_eq!(
            resolve_provider_in(HarnessId::Cursor, directory.path().to_str()).unwrap(),
            candidate
        );
        assert!(!sentinel.exists());
    }

    #[test]
    fn launches_script_entry_points_with_node_and_refuses_custom_windows_wrappers() {
        assert_eq!(
            provider_launch("/bin/tool.cjs", &["--version".into()], "macos").unwrap(),
            Launch {
                command: "node".into(),
                args: vec!["/bin/tool.cjs".into(), "--version".into()],
            }
        );
        assert_eq!(
            provider_launch("/bin/claude", &[], "macos")
                .unwrap()
                .command,
            "/bin/claude"
        );
        assert_eq!(
            provider_launch("C:\\tools\\custom.cmd", &[], "windows").unwrap_err(),
            "Unsupported Windows provider launcher"
        );
    }
}
