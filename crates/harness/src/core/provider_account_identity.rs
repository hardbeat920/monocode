//! Port of src/features/providers/model/providerAccountIdentity.ts: the
//! identity a provider CLI cached on disk after sign-in.
//!
//! The TypeScript read it through the `provider_account_identity` command,
//! which `monocode-integrations` implements. This crate cannot depend on
//! that, so the reader is a parameter. The React hook
//! `useProviderAccountIdentities` becomes
//! [`load_provider_account_identities`].

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use monocode_core::harness::HarnessId;

use super::provider_accounts::ProviderAccount;

/// `ProviderAccountIdentity`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ProviderAccountIdentity {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organization: Option<String>,
}

/// Reads one account's identity: `provider_account_identity(provider, accountId)`.
pub type IdentityReader<'a> =
    dyn Fn(HarnessId, &str) -> Result<Option<ProviderAccountIdentity>, String> + Send + Sync + 'a;

/// `readProviderAccountIdentity`. A failed read is `None`.
pub fn read_provider_account_identity(
    read: &IdentityReader<'_>,
    provider: HarnessId,
    account_id: &str,
) -> Option<ProviderAccountIdentity> {
    read(provider, account_id).ok().flatten()
}

static PERSONAL_ORG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"['’]s Organization$").unwrap());

/// `identityOrganizationTag`: "Personal" for Claude's default
/// "<name>'s Organization".
pub fn identity_organization_tag(identity: Option<&ProviderAccountIdentity>) -> Option<String> {
    let name = identity?.organization.as_deref()?.trim();
    if name.is_empty() {
        return None;
    }
    Some(if PERSONAL_ORG.is_match(name) {
        "Personal".into()
    } else {
        name.to_string()
    })
}

/// `identityKey`.
pub fn identity_key(account: &ProviderAccount) -> String {
    format!("{}:{}", account.provider, account.id)
}

/// The data side of `useProviderAccountIdentities`: every account's
/// identity, keyed by [`identity_key`]. Run it off the UI thread.
pub fn load_provider_account_identities(
    read: &IdentityReader<'_>,
    accounts: &[ProviderAccount],
) -> HashMap<String, Option<ProviderAccountIdentity>> {
    accounts
        .iter()
        .map(|account| {
            (
                identity_key(account),
                read_provider_account_identity(read, account.provider, &account.id),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn org(name: &str) -> ProviderAccountIdentity {
        ProviderAccountIdentity {
            organization: Some(name.into()),
            ..Default::default()
        }
    }

    #[test]
    fn tags_the_default_claude_organization_as_personal() {
        assert_eq!(
            identity_organization_tag(Some(&org("Alice's Organization"))).as_deref(),
            Some("Personal")
        );
        assert_eq!(
            identity_organization_tag(Some(&org("Alice’s Organization"))).as_deref(),
            Some("Personal")
        );
        assert_eq!(
            identity_organization_tag(Some(&org(" Acme "))).as_deref(),
            Some("Acme")
        );
        assert_eq!(identity_organization_tag(Some(&org("  "))), None);
        assert_eq!(identity_organization_tag(None), None);
    }

    #[test]
    fn loads_identities_by_key_and_hides_read_errors() {
        let accounts = vec![
            ProviderAccount::new("default", HarnessId::Claude, "Default account"),
            ProviderAccount::new("account-x", HarnessId::Codex, "Work"),
        ];
        let read = |provider: HarnessId, _id: &str| match provider {
            HarnessId::Claude => Ok(Some(ProviderAccountIdentity {
                email: Some("a@b.c".into()),
                ..Default::default()
            })),
            _ => Err("unreadable".to_string()),
        };
        let identities = load_provider_account_identities(&read, &accounts);
        assert_eq!(
            identities["claude:default"]
                .as_ref()
                .unwrap()
                .email
                .as_deref(),
            Some("a@b.c")
        );
        assert_eq!(identities["codex:account-x"], None);
    }
}
