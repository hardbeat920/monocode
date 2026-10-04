//! Fake provider executables for tests that run real processes.
//!
//! The process supervisor accepts a provider binary only when its file name
//! is the provider's, its contents identify it, and `--version` prints a
//! version. Unix fixtures are executable Node scripts. Windows fixtures are
//! native launchers with the script beside them. These tests require Node;
//! Windows also uses the Rust compiler to build the launcher once.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use monocode_core::HarnessId;
use monocode_remote::host::protocol::{REMOTE_PROVIDERS, RemoteProvider};

/// Markers the supervisor's identity checks look for, and the answers to
/// its `--version` and `--help` probes.
const PRELUDE: &str = r#"#!/usr/bin/env node
// Fixture markers: pi-coding-agent vercel-labs/fx xai-grok
if (process.argv.slice(2).join(' ') === '--version') { console.log('1.0.0 claude codex hermes fixture'); process.exit(0); }
if (process.argv.slice(2).join(' ') === '--help') { console.log('usage: --mode rpc'); process.exit(0); }
"#;

/// The file name the supervisor expects for `provider`.
pub fn binary_name(provider: RemoteProvider) -> &'static str {
    match provider {
        HarnessId::Cursor => "cursor-agent",
        HarnessId::Antigravity => "agy_acp_server.par",
        other => other.as_str(),
    }
}

/// Writes `body` as an executable named for `provider` in its own folder
/// under `directory`.
pub fn fake_provider(directory: &Path, provider: RemoteProvider, body: &str) -> PathBuf {
    fake_provider_script(directory, provider, &format!("{PRELUDE}{body}"))
}

/// Writes a complete fixture script, including its own version response.
pub fn fake_provider_script(directory: &Path, provider: RemoteProvider, script: &str) -> PathBuf {
    let folder = directory.join(format!("bin-{}", provider.as_str()));
    std::fs::create_dir_all(&folder).unwrap();
    #[cfg(windows)]
    {
        let path = folder.join(format!("{}.exe", binary_name(provider)));
        std::fs::write(folder.join("fixture.js"), script).unwrap();
        std::fs::write(&path, windows_launcher()).unwrap();
        path
    }
    #[cfg(not(windows))]
    {
        let path = folder.join(binary_name(provider));
        std::fs::write(&path, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }
}

#[cfg(windows)]
fn windows_launcher() -> &'static [u8] {
    use std::sync::OnceLock;

    static LAUNCHER: OnceLock<Vec<u8>> = OnceLock::new();
    LAUNCHER.get_or_init(|| {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("fixture.rs");
        let executable = directory.path().join("fixture.exe");
        // The resolver checks the executable's identity, then runs its real
        // version/help probes. Keep those checks active for the test fixture.
        std::fs::write(
            &source,
            r#"fn main() {
    std::hint::black_box("pi-coding-agent vercel-labs/fx xai-grok");
    let executable = std::env::current_exe().expect("fixture executable path");
    let script = executable.parent().unwrap().join("fixture.js");
    let status = std::process::Command::new("node.exe")
        .arg(script)
        .args(std::env::args_os().skip(1))
        .status()
        .expect("Node is required for host provider tests");
    std::process::exit(status.code().unwrap_or(1));
}
"#,
        )
        .unwrap();
        let result = std::process::Command::new("rustc")
            .args([
                "--edition=2021",
                "--crate-name=monocode_host_fixture",
                "-C",
                "debuginfo=0",
                "-C",
                "target-feature=+crt-static",
            ])
            .arg(&source)
            .arg("-o")
            .arg(&executable)
            .output()
            .expect("Rust is required to compile the Windows provider fixture");
        assert!(
            result.status.success(),
            "Windows provider fixture compilation failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        std::fs::read(executable).unwrap()
    })
}

/// [`fake_provider`] for every provider.
pub fn fake_providers(directory: &Path, body: &str) -> HashMap<RemoteProvider, PathBuf> {
    REMOTE_PROVIDERS
        .into_iter()
        .map(|provider| (provider, fake_provider(directory, provider, body)))
        .collect()
}

/// `vi.waitFor`.
pub fn wait_for(what: &str, timeout: std::time::Duration, condition: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + timeout;
    while !condition() {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
