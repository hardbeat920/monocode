//! Port of the types in src/features/sessions/model/handoff.ts.
//!
//! The handoff flow itself (recaps, prompts, switch planning) stays with the
//! engine.

use serde::{Deserialize, Serialize};

use crate::harness::HarnessId;
use crate::session::PendingHarnessSwitch;

/// `HANDOFF_TITLE`.
pub const HANDOFF_TITLE: &str = "Handoff";

/// Composer chip. The recap is injected on send so the user can add context first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoffComposerCard {
    pub from: HarnessId,
    pub to: HarnessId,
    pub brief: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<i64>,
}

/// `ComposerSwitchPlan`: what changing the composer's provider does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum ComposerSwitchPlan {
    /// Same provider: only the model changes.
    #[serde(rename = "model")]
    Model,
    /// No user turn yet: forget the old provider.
    #[serde(rename = "empty")]
    Empty { forget: HarnessId },
    /// Back to the provider a pending switch came from.
    #[serde(rename = "revert", rename_all = "camelCase")]
    Revert {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        restore_provider_session_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        restore_provider_account_id: Option<String>,
    },
    /// Hand off on the next send.
    #[serde(rename = "arm")]
    Arm { pending: PendingHarnessSwitch },
}
