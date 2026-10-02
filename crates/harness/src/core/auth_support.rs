//! Port of src/integrations/harness/core/authSupport.ts: which providers
//! have one login command, and how to spot an authentication failure.

use std::sync::LazyLock;

use regex::Regex;

use monocode_core::block::{Block, BlockNotice, BlockRole};
use monocode_core::harness::HarnessId;

/// `LOGIN_ARGS`: account-level login commands that can run without an
/// interactive provider picker. OpenCode, Pi, and omp authenticate individual
/// upstream providers, so one generic browser button would mislead.
fn login_args(harness: HarnessId) -> Option<&'static [&'static str]> {
    match harness {
        HarnessId::Claude => Some(&["auth", "login"]),
        HarnessId::Codex => Some(&["login"]),
        HarnessId::Cursor => Some(&["login"]),
        HarnessId::Grok => Some(&["login", "--oauth"]),
        // MonoCode uses fx through Vercel AI Gateway. Choosing it explicitly
        // keeps `fx login` from waiting on a TTY-only provider picker.
        HarnessId::Fx => Some(&["login", "vercel"]),
        _ => None,
    }
}

/// `supportsHarnessLogin`.
pub fn supports_harness_login(harness: HarnessId) -> bool {
    login_args(harness).is_some()
}

/// `harnessLoginArgs`. For login execution, settings copy, and tests.
pub fn harness_login_args(harness: HarnessId) -> Option<&'static [&'static str]> {
    login_args(harness)
}

// JavaScript `\b` is ASCII-only; these use Unicode word boundaries, which
// only differ next to non-ASCII letters.
static AUTH_ERRORS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"(?i)\bauthentication required\b",
        r"(?i)\bnot (?:authenticated|signed in|logged in)\b",
        r"(?i)\b(?:sign-in|login|session) (?:has )?expired\b",
        r"(?i)\bplease (?:sign|log) in\b",
        r#"(?i)\brun [`'"]?\S+ (?:auth )?login\b"#,
    ]
    .into_iter()
    .map(|pattern| Regex::new(pattern).unwrap())
    .collect()
});

/// `isHarnessAuthError`: the provider error asks the user to sign in again.
pub fn is_harness_auth_error(message: &str) -> bool {
    AUTH_ERRORS.iter().any(|pattern| pattern.is_match(message))
}

/// `latestTurnNeedsHarnessLogin`: the active turn ended on a provider
/// authentication failure.
pub fn latest_turn_needs_harness_login(blocks: &[Block]) -> bool {
    for block in blocks.iter().rev() {
        if block.role == BlockRole::User {
            return false;
        }
        if block.role == BlockRole::System
            && block.notice == Some(BlockNotice::Error)
            && is_harness_auth_error(&block.text)
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advertises_only_deterministic_account_login_commands() {
        assert_eq!(
            harness_login_args(HarnessId::Claude),
            Some(&["auth", "login"][..])
        );
        assert_eq!(harness_login_args(HarnessId::Codex), Some(&["login"][..]));
        assert_eq!(harness_login_args(HarnessId::Cursor), Some(&["login"][..]));
        assert_eq!(
            harness_login_args(HarnessId::Grok),
            Some(&["login", "--oauth"][..])
        );
        assert_eq!(
            harness_login_args(HarnessId::Fx),
            Some(&["login", "vercel"][..])
        );
        assert!(!supports_harness_login(HarnessId::Opencode));
        assert!(!supports_harness_login(HarnessId::Pi));
        assert!(!supports_harness_login(HarnessId::Omp));
    }

    #[test]
    fn recognizes_provider_authentication_failures_without_matching_unrelated_errors() {
        assert!(is_harness_auth_error(
            "Authentication required\n\nGrok Build is not signed in. Run `grok login` in a terminal."
        ));
        assert!(is_harness_auth_error("Claude sign-in expired"));
        assert!(!is_harness_auth_error("Provider request timed out"));
    }

    #[test]
    fn only_carries_an_authentication_failure_through_its_current_turn() {
        let mut error = Block::new("e", BlockRole::System, "Grok Build is not signed in.");
        error.notice = Some(BlockNotice::Error);
        assert!(latest_turn_needs_harness_login(std::slice::from_ref(
            &error
        )));
        assert!(!latest_turn_needs_harness_login(&[
            error,
            Block::new("u", BlockRole::User, "Retry this"),
        ]));
    }
}
