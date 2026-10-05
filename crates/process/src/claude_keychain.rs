//! Claude Code credentials in the macOS Keychain. Moved from
//! src-tauri/src/rate_limits.rs so that removing a provider account and
//! reading usage share one service name.

use std::time::Duration;

use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

pub const KEYCHAIN_TIMEOUT: Duration = Duration::from_secs(5);
const LEGACY_KEYCHAIN_SERVICE: &str = "Claude Code-credentials";

pub fn claude_keychain_service(config_dir: Option<&std::path::Path>) -> String {
    let Some(config_dir) = config_dir else {
        return LEGACY_KEYCHAIN_SERVICE.into();
    };
    // Claude Code hashes the exact, NFC-normalized selector string and uses
    // the first eight lowercase hex characters as its Keychain service suffix.
    let selector: String = config_dir.to_string_lossy().nfc().collect();
    let digest = Sha256::digest(selector.as_bytes());
    let suffix = format!("{digest:x}");
    format!("{LEGACY_KEYCHAIN_SERVICE}-{}", &suffix[..8])
}

pub fn delete_claude_keychain_credentials(config_dir: &std::path::Path) -> Result<(), String> {
    let service = claude_keychain_service(Some(config_dir));
    let args = vec![
        "delete-generic-password".into(),
        "-s".into(),
        service.clone(),
    ];
    security_delete(&args).map_err(|error| {
        format!("Could not remove the Claude credentials from Keychain ({service}): {error}")
    })
}

fn security_delete(args: &[String]) -> Result<(), String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::Instant;

    let mut child = Command::new("security")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut error = String::new();
                if let Some(mut stderr) = child.stderr.take() {
                    let _ = stderr.read_to_string(&mut error);
                }
                if status.success()
                    || error.contains("could not be found")
                    || error.contains("specified item could not be found")
                {
                    return Ok(());
                }
                let detail = error.trim();
                return Err(if detail.is_empty() {
                    format!("security exited with {status}")
                } else {
                    detail.to_string()
                });
            }
            Ok(None) if started.elapsed() < KEYCHAIN_TIMEOUT => {
                std::thread::sleep(Duration::from_millis(25));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("security timed out".into());
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_config_dir_selects_claudes_hashed_keychain_service() {
        assert_eq!(
            claude_keychain_service(Some(std::path::Path::new("/tmp/profile"))),
            "Claude Code-credentials-902e721c"
        );
        assert_eq!(claude_keychain_service(None), "Claude Code-credentials");
    }
}
