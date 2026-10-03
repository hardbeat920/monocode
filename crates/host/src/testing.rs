//! Fake provider executables for tests that run real processes.
//!
//! The process supervisor accepts a provider binary only when its file name
//! is the provider's, its contents identify it, and `--version` prints a
//! version. Each fake is the same Node script written under every name, with
//! those markers in front of the script's own behavior.

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
    let folder = directory.join(format!("bin-{}", provider.as_str()));
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join(binary_name(provider));
    std::fs::write(&path, format!("{PRELUDE}{body}")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    path
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
