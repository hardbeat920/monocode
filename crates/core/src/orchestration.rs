//! Port of the proposal types and the pure card helpers in
//! src/features/orchestration/model/orchestrationPlan.ts.
//!
//! The proposal validators and prompts stay with the orchestration engine.

use serde::{Deserialize, Serialize};

use crate::block::{Block, BlockRole, Extra, ModelSettings};
use crate::harness::HarnessId;
use crate::session::Session;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrchestrationChoice {
    pub harness: HarnessId,
    pub model: String,
    pub name: String,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrchestrationSettings {
    pub choices: Vec<OrchestrationChoice>,
    pub max_workers: i64,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposedTask {
    pub id: String,
    pub title: String,
    pub prompt: String,
    pub harness: HarnessId,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_settings: Option<ModelSettings>,
    pub files: Vec<String>,
    pub depends_on: Vec<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OrchestrationProposalStatus {
    #[serde(rename = "planning")]
    Planning,
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "invalid")]
    Invalid,
    #[serde(rename = "starting")]
    Starting,
    #[serde(rename = "approved")]
    Approved,
}

/// An orchestrator's assignment card, saved on its plan block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrchestrationProposal {
    /// Always 1 today.
    pub version: i64,
    pub lead_id: String,
    /// Project identity retained for history and proposal ownership.
    pub cwd: String,
    /// Concrete checkout inspected while preparing this proposal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkout_cwd: Option<String>,
    pub request: String,
    pub author: OrchestrationChoice,
    pub settings: OrchestrationSettings,
    pub status: OrchestrationProposalStatus,
    pub title: String,
    pub summary: String,
    pub tasks: Vec<ProposedTask>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Kept only for invalid cards so a retry can repair the response directly.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `proposalMarkdown`: the card as plain markdown, stored in the block text.
pub fn proposal_markdown(proposal: &OrchestrationProposal) -> String {
    let mut parts = vec![format!("# {}", proposal.title), proposal.summary.clone()];
    for task in &proposal.tasks {
        let name = proposal
            .settings
            .choices
            .iter()
            .find(|choice| choice.harness == task.harness && choice.model == task.model)
            .map(|choice| choice.name.as_str())
            .unwrap_or(task.model.as_str());
        let depends = if task.depends_on.is_empty() {
            "None".to_string()
        } else {
            task.depends_on.join(", ")
        };
        parts.push(format!(
            "## {}\n{} · {}\n{}\nFiles: {}\nDepends on: {}",
            task.title,
            task.harness,
            name,
            task.prompt,
            task.files.join(", "),
            depends
        ));
    }
    parts.join("\n\n")
}

/// `withOrchestrationProposal`: put a proposal on the block with `block_id`.
pub fn with_orchestration_proposal(
    session: &Session,
    block_id: &str,
    proposal: &OrchestrationProposal,
) -> Session {
    let mut next = session.clone();
    for block in &mut next.blocks {
        if block.id == block_id {
            block.text = proposal_markdown(proposal);
            block.orchestration = Some(proposal.clone());
            block.streaming = Some(proposal.status == OrchestrationProposalStatus::Planning);
        }
    }
    next
}

/// `proposalBlock`: a new streaming plan block for a proposal.
pub fn proposal_block(id: impl Into<String>, proposal: &OrchestrationProposal) -> Block {
    Block {
        orchestration: Some(proposal.clone()),
        streaming: Some(true),
        ..Block::new(id, BlockRole::Plan, proposal_markdown(proposal))
    }
}

/// `restoreOrchestrationProposal`: a reload must never turn a half-generated
/// card into an executable plan.
pub fn restore_orchestration_proposal(value: &OrchestrationProposal) -> OrchestrationProposal {
    let mut next = value.clone();
    match value.status {
        OrchestrationProposalStatus::Planning => {
            next.status = OrchestrationProposalStatus::Invalid;
            next.error = Some("Planning was interrupted. Generate the assignments again.".into());
        }
        OrchestrationProposalStatus::Starting => {
            next.status = OrchestrationProposalStatus::Ready;
        }
        _ => {}
    }
    next
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn proposal(status: &str) -> OrchestrationProposal {
        serde_json::from_value(json!({
            "version": 1,
            "leadId": "lead",
            "cwd": "/repo",
            "request": "Do it",
            "author": { "harness": "claude", "model": "claude:opus-5", "name": "Opus 5" },
            "settings": {
                "maxWorkers": 2,
                "choices": [{ "harness": "codex", "model": "codex:gpt-5", "name": "GPT-5" }]
            },
            "status": status,
            "title": "Ship",
            "summary": "Two steps.",
            "tasks": [
                { "id": "a", "title": "First", "prompt": "Do a", "harness": "codex", "model": "codex:gpt-5", "files": ["src"], "dependsOn": [] },
                { "id": "b", "title": "Second", "prompt": "Do b", "harness": "claude", "model": "claude:x", "files": ["."], "dependsOn": ["a"] }
            ]
        }))
        .unwrap()
    }

    #[test]
    fn renders_the_card_as_markdown() {
        assert_eq!(
            proposal_markdown(&proposal("ready")),
            "# Ship\n\nTwo steps.\n\n## First\ncodex · GPT-5\nDo a\nFiles: src\nDepends on: None\n\n## Second\nclaude · claude:x\nDo b\nFiles: .\nDepends on: a"
        );
    }

    #[test]
    fn never_restores_a_half_generated_card_as_executable() {
        let planning = restore_orchestration_proposal(&proposal("planning"));
        assert_eq!(planning.status, OrchestrationProposalStatus::Invalid);
        assert!(planning.error.is_some());
        let starting = restore_orchestration_proposal(&proposal("starting"));
        assert_eq!(starting.status, OrchestrationProposalStatus::Ready);
        let approved = restore_orchestration_proposal(&proposal("approved"));
        assert_eq!(approved, proposal("approved"));
    }
}
