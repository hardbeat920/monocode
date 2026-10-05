//! Port of src/integrations/harness/core/textHarness.ts: pick the harness
//! that writes titles, commit messages, and pull request text.
//!
//! The TypeScript read the installer probe from module state; here the
//! caller passes `is_available`, usually
//! `HarnessAvailabilityStore::is_harness_available`.

use anyhow::Result;

use monocode_core::harness::HarnessId;

use super::registry::{GeneratedPrContent, HarnessRegistry};
use super::task::AbortSignal;

/// `TEXT_HARNESSES`, in preference order.
pub const TEXT_HARNESSES: [HarnessId; 5] = [
    HarnessId::Claude,
    HarnessId::Cursor,
    HarnessId::Codex,
    HarnessId::Grok,
    HarnessId::Opencode,
];

/// `pickTextHarness`.
pub fn pick_text_harness(
    preferred: Option<HarnessId>,
    is_available: impl Fn(HarnessId) -> bool,
) -> HarnessId {
    let preferred = preferred.filter(|id| TEXT_HARNESSES.contains(id));
    let ordered: Vec<HarnessId> = match preferred {
        Some(first) => std::iter::once(first)
            .chain(TEXT_HARNESSES.into_iter().filter(|id| *id != first))
            .collect(),
        None => TEXT_HARNESSES.to_vec(),
    };
    ordered
        .into_iter()
        .find(|id| is_available(*id))
        .or(preferred)
        .unwrap_or(HarnessId::Cursor)
}

/// `warmupText`.
pub async fn warmup_text(
    registry: &HarnessRegistry,
    cwd: &str,
    preferred: Option<HarnessId>,
    is_available: impl Fn(HarnessId) -> bool,
) -> Result<()> {
    registry
        .warmup_harness_text(pick_text_harness(preferred, is_available), cwd)
        .await
}

/// `generateCommitMessage`.
pub async fn generate_commit_message(
    registry: &HarnessRegistry,
    cwd: &str,
    preferred: Option<HarnessId>,
    signal: Option<AbortSignal>,
    is_available: impl Fn(HarnessId) -> bool,
) -> Result<String> {
    registry
        .generate_harness_commit_message(pick_text_harness(preferred, is_available), cwd, signal)
        .await
}

/// `generatePrContent`.
pub async fn generate_pr_content(
    registry: &HarnessRegistry,
    cwd: &str,
    preferred: Option<HarnessId>,
    is_available: impl Fn(HarnessId) -> bool,
) -> Result<Option<GeneratedPrContent>> {
    registry
        .generate_harness_pr_content(pick_text_harness(preferred, is_available), cwd)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_the_requested_text_harness_when_installed() {
        let only = |installed: &'static [HarnessId]| move |id| installed.contains(&id);
        assert_eq!(
            pick_text_harness(
                Some(HarnessId::Codex),
                only(&[HarnessId::Claude, HarnessId::Codex])
            ),
            HarnessId::Codex
        );
        assert_eq!(
            pick_text_harness(None, only(&[HarnessId::Grok])),
            HarnessId::Grok
        );
        // A preferred harness that cannot write text is ignored.
        assert_eq!(
            pick_text_harness(Some(HarnessId::Pi), only(&[HarnessId::Opencode])),
            HarnessId::Opencode
        );
    }

    #[test]
    fn falls_back_without_any_installed_cli() {
        assert_eq!(
            pick_text_harness(Some(HarnessId::Grok), |_| false),
            HarnessId::Grok
        );
        assert_eq!(
            pick_text_harness(Some(HarnessId::Fx), |_| false),
            HarnessId::Cursor
        );
        assert_eq!(pick_text_harness(None, |_| false), HarnessId::Cursor);
    }
}
