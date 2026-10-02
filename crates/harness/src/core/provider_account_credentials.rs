//! Port of src/features/providers/model/providerAccountCredentials.ts:
//! remove a named profile's native credentials before its UI metadata.

use std::path::PathBuf;

use anyhow::{Result, anyhow};

use monocode_core::harness::HarnessId;
use monocode_process::harness::{HarnessHost, provider_account_remove};

/// `removeProviderAccountCredentials`: kill the profile's children, delete
/// its Keychain entry (Claude on macOS) and its directory. Runs on smol's
/// blocking pool.
pub async fn remove_provider_account_credentials(
    host: &HarnessHost,
    data_dir: PathBuf,
    provider: HarnessId,
    account_id: &str,
) -> Result<()> {
    let host = host.clone();
    let account_id = account_id.to_string();
    smol::unblock(move || {
        provider_account_remove(&host, &data_dir, provider.as_str().to_string(), account_id)
    })
    .await
    .map_err(|error| anyhow!(error))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_the_default_account_and_unsupported_providers() {
        let host = HarnessHost::default();
        let dir = std::env::temp_dir();
        let error = smol::block_on(remove_provider_account_credentials(
            &host,
            dir.clone(),
            HarnessId::Claude,
            "default",
        ))
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "The default provider account cannot be removed"
        );
        let error = smol::block_on(remove_provider_account_credentials(
            &host,
            dir,
            HarnessId::Cursor,
            "account-x",
        ))
        .unwrap_err();
        assert!(error.to_string().contains("not supported"));
    }
}
