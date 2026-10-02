//! Port of the Session type and its pure helpers in
//! src/features/sessions/model/session.ts, plus `usageLimitResumeDue` from
//! usageLimit.ts.
//!
//! The TypeScript created ids with `crypto.randomUUID()` and read provider
//! defaults from localStorage. This crate has neither, so the constructors
//! take the new id and a `ModelEnv`.

use serde::{Deserialize, Serialize};

use crate::attachment::Attachment;
use crate::block::{Block, BlockRole, Extra, ModelSettings, TurnIntent};
use crate::context_usage::{ContextUsage, drop_context_window};
use crate::handoff::HandoffComposerCard;
use crate::harness::{DEFAULT_RUNTIME_MODE, HarnessId, RuntimeMode};
use crate::inbox::{InboxAskContext, InboxComposerCard, LinkedWorkItemUpdateCard, WorkItemKind};
use crate::js;
use crate::models::ModelEnv;
use crate::notes::NoteComposerCard;
use crate::task_list::split_lines;
use crate::user_question::UserQuestionPrompt;

/// A follow-up waiting for the current turn. In-memory only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueuedMessage {
    pub id: String,
    pub text: String,
    pub attachments: Vec<Attachment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note_card: Option<NoteComposerCard>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handoff_card: Option<HandoffComposerCard>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent: Option<TurnIntent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MessageQueueStatus {
    #[serde(rename = "active")]
    Active,
    #[serde(rename = "paused")]
    Paused,
    #[serde(rename = "resuming")]
    Resuming,
}

/// The provider stopped the last turn at a usage limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageLimit {
    /// Epoch ms when the provider's window resets, once known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<i64>,
    /// Send a continue turn once the window resets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resume_at_reset: Option<bool>,
}

/// The composer switched providers while the previous child is still live.
/// The handoff runs on the next send, not on the picker change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingHarnessSwitch {
    pub from: HarnessId,
    pub from_model: String,
    pub from_settings: ModelSettings,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_provider_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_provider_account_id: Option<String>,
}

/// One GitHub issue or pull request associated with a coding session.
/// Stored in the `sessions.linked_work_item_json` column.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkedWorkItem {
    pub kind: WorkItemKind,
    pub repo: String,
    pub number: i64,
    pub url: String,
    #[serde(flatten)]
    pub extra: Extra,
}

/// The blank-composer choice of working copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WorkspaceMode {
    #[serde(rename = "current")]
    Current,
    #[serde(rename = "worktree")]
    Worktree,
}

/// `EditedResendRejection`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditedResendRejection {
    /// The provider removed the old turn, so retry as a normal unsent prompt.
    pub provider_rewound: bool,
}

/// `ComposerTurnOptions`, without the `onResendRejected` callback.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposerTurnOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent: Option<TurnIntent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resend_edited: Option<bool>,
    /// Promote an existing unsent transcript block instead of appending a turn.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub draft_block_id: Option<String>,
}

/// One conversation with an agent.
///
/// The store saves the persisted fields as `sessions` columns, not as one
/// JSON value. Fields marked in-memory never reach the database.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    /// Receipt for an acknowledged floating-composer handoff.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quick_launch_accepted: Option<bool>,
    /// Internal worker shown in its lead's panel rather than a workspace tab.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orchestration_lead_id: Option<String>,
    /// Temporary Inbox conversation: shares the runtime, never saved as a session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inbox_ask: Option<InboxAskContext>,
    pub id: String,
    pub harness: HarnessId,
    pub model: String,
    pub model_settings: ModelSettings,
    pub runtime_mode: RuntimeMode,
    pub title: String,
    /// Project and working directory for this session.
    pub cwd: String,
    pub blocks: Vec<Block>,
    /// A harness turn is in flight.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub busy: Option<bool>,
    /// What the live turn waits on after the agent yielded with work still
    /// running in the background. In-memory only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background_tasks: Option<Vec<String>>,
    /// Follow-ups waiting for the current turn. In-memory only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queued_messages: Option<Vec<QueuedMessage>>,
    /// Paused after the user stops the current turn; resuming waits for the continued turn.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue_status: Option<MessageQueueStatus>,
    /// Blocks auto-dispatch while this queued row is being edited. In-memory only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub editing_queued_message_id: Option<String>,
    /// The last turn hit a provider usage limit; the next send clears it. In-memory only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_limit: Option<UsageLimit>,
    /// Provider-side conversation id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_session_id: Option<String>,
    /// Named local credential profile used by Claude or Codex.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_account_id: Option<String>,
    /// Context-window level reported by the harness. `None` until it reports.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<ContextUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_switch: Option<PendingHarnessSwitch>,
    /// Last known branch in the session's working copy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// Selected working copy. `cwd` stays the project identity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_cwd: Option<String>,
    /// Blank-composer choice, consumed when the first turn starts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_mode: Option<WorkspaceMode>,
    /// Base ref for a worktree created on first send.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_base: Option<String>,
    /// Internal guard while the first turn creates its selected worktree.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_preparing: Option<bool>,
    /// Select a working copy before continuing after the previous one was deleted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_removed: Option<bool>,
    /// One-shot composer text when opening a session from Inbox.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub composer_seed: Option<String>,
    /// Inbox issue or pull request chip above the composer. In-memory, one-shot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inbox_card: Option<InboxComposerCard>,
    /// GitHub issue or pull request shown on the persisted session card.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linked_work_item: Option<LinkedWorkItem>,
    /// Automation that created or last launched this session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub automation_id: Option<String>,
    /// New linked-item activity above the composer. In-memory, one-shot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linked_work_item_update_card: Option<LinkedWorkItemUpdateCard>,
    /// Note chip above the composer. In-memory, one-shot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note_card: Option<NoteComposerCard>,
    /// Handoff chip above the composer. In-memory, one-shot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handoff_card: Option<HandoffComposerCard>,
    /// Live clarifying questions. In-memory; request ids do not survive restarts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_question: Option<UserQuestionPrompt>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Session {
    /// A blank session with every optional field unset.
    pub fn blank(
        id: impl Into<String>,
        harness: HarnessId,
        model: impl Into<String>,
        cwd: impl Into<String>,
    ) -> Self {
        Self {
            quick_launch_accepted: None,
            orchestration_lead_id: None,
            inbox_ask: None,
            id: id.into(),
            harness,
            model: model.into(),
            model_settings: ModelSettings::new(),
            runtime_mode: DEFAULT_RUNTIME_MODE,
            title: harness.label().to_string(),
            cwd: cwd.into(),
            blocks: Vec::new(),
            busy: None,
            background_tasks: None,
            queued_messages: None,
            queue_status: None,
            editing_queued_message_id: None,
            usage_limit: None,
            provider_session_id: None,
            provider_account_id: None,
            context: None,
            pending_switch: None,
            branch: None,
            worktree_cwd: None,
            workspace_mode: None,
            worktree_base: None,
            worktree_preparing: None,
            worktree_removed: None,
            composer_seed: None,
            inbox_card: None,
            linked_work_item: None,
            automation_id: None,
            linked_work_item_update_card: None,
            note_card: None,
            handoff_card: None,
            pending_question: None,
            extra: Extra::new(),
        }
    }

    /// `session.busy` is truthy.
    pub fn is_busy(&self) -> bool {
        self.busy == Some(true)
    }
}

/// `newSession`. The TypeScript defaults were `harness = "claude"`,
/// `cwd = "~"`, and `runtimeMode = DEFAULT_RUNTIME_MODE`.
pub fn new_session(
    env: &ModelEnv<'_>,
    id: impl Into<String>,
    harness: HarnessId,
    cwd: &str,
    model: Option<&str>,
    runtime_mode: Option<RuntimeMode>,
    model_settings: Option<&ModelSettings>,
) -> Session {
    let preferred;
    let model = match model {
        Some(model) => model,
        None => {
            preferred = env.preferred_model_id(harness);
            preferred.as_str()
        }
    };
    let resolved = env.catalog.resolve_model(harness, Some(model));
    let mut session = Session::blank(id, harness, resolved.id.clone(), cwd);
    session.model_settings = env.preferred_model_settings(&resolved, model_settings);
    session.runtime_mode = runtime_mode.unwrap_or(DEFAULT_RUNTIME_MODE);
    session
}

/// `newDefaultSession`: a new conversation using the Providers defaults.
pub fn new_default_session(
    env: &ModelEnv<'_>,
    id: impl Into<String>,
    cwd: &str,
    runtime_mode: Option<RuntimeMode>,
) -> Session {
    let choice = env.default_session_choice(Some(cwd));
    new_session(
        env,
        id,
        choice.harness,
        cwd,
        Some(&choice.model),
        runtime_mode,
        None,
    )
}

/// `projectSessionChoice`: the project's own default provider and model win
/// over the seed. When the project has neither, the seed's are carried. A
/// provider the project hides is swapped for its first enabled one.
fn project_session_choice(
    env: &ModelEnv<'_>,
    seed: Option<(HarnessId, &str)>,
    cwd: &str,
) -> (HarnessId, Option<String>) {
    let project = env.projects.load(Some(cwd));
    let seed_harness = seed
        .map(|(harness, _)| harness)
        .unwrap_or(HarnessId::Claude);
    let harness =
        env.first_enabled_harness(Some(cwd), project.default_harness.unwrap_or(seed_harness));
    let model = project
        .models
        .as_ref()
        .and_then(|models| models.get(&harness).cloned())
        .or_else(|| {
            (project.default_harness == Some(harness))
                .then(|| project.default_model.clone())
                .flatten()
        })
        .or_else(|| {
            (project.default_harness.is_none() && harness == seed_harness)
                .then(|| seed.map(|(_, model)| model.to_string()))
                .flatten()
        });
    (harness, model)
}

/// `newSessionForProject`: a new conversation for a project, adopting the
/// project's provider defaults.
pub fn new_session_for_project(
    env: &ModelEnv<'_>,
    id: impl Into<String>,
    seed: Option<&Session>,
    cwd: &str,
) -> Session {
    let (harness, model) = project_session_choice(
        env,
        seed.map(|seed| (seed.harness, seed.model.as_str())),
        cwd,
    );
    let carries_seed = seed.is_some_and(|seed| {
        model.as_deref() == Some(seed.model.as_str()) && harness == seed.harness
    });
    new_session(
        env,
        id,
        harness,
        cwd,
        model.as_deref(),
        seed.map(|seed| seed.runtime_mode),
        if carries_seed {
            seed.map(|seed| &seed.model_settings)
        } else {
            None
        },
    )
}

/// `retargetSessionToProject`: move an existing (usually blank) session to a
/// project, adopting its provider defaults while keeping the id, blocks, and
/// composer seed.
pub fn retarget_session_to_project(env: &ModelEnv<'_>, session: &Session, cwd: &str) -> Session {
    let (harness, model) =
        project_session_choice(env, Some((session.harness, session.model.as_str())), cwd);
    let carries_seed =
        model.as_deref() == Some(session.model.as_str()) && harness == session.harness;
    let model = model.unwrap_or_else(|| env.preferred_model_id(harness));
    let resolved = env.catalog.resolve_model(harness, Some(&model));
    let mut next = session.clone();
    next.cwd = cwd.to_string();
    next.harness = harness;
    next.model = resolved.id.clone();
    next.model_settings = env.preferred_model_settings(
        &resolved,
        if carries_seed {
            Some(&session.model_settings)
        } else {
            None
        },
    );
    next.title = harness.label().to_string();
    if harness != session.harness {
        next.provider_session_id = None;
        next.provider_account_id = None;
    }
    if resolved.id != session.model {
        next.context = drop_context_window(session.context.as_ref());
    }
    next
}

/// `newSessionLike`: a new conversation carrying another session's harness,
/// model, and settings.
pub fn new_session_like(
    env: &ModelEnv<'_>,
    id: impl Into<String>,
    seed: Option<&Session>,
    cwd: &str,
) -> Session {
    new_session(
        env,
        id,
        seed.map(|seed| seed.harness).unwrap_or(HarnessId::Claude),
        cwd,
        seed.map(|seed| seed.model.as_str()),
        seed.map(|seed| seed.runtime_mode),
        seed.map(|seed| &seed.model_settings),
    )
}

/// `titleFromPrompt`: the first line of a prompt, truncated for the tab strip.
pub fn title_from_prompt(prompt: &str, harness: HarnessId, attachments: &[Attachment]) -> String {
    let line = split_lines(js::trim(prompt))
        .first()
        .map(|line| js::trim(line))
        .unwrap_or("");
    let from_files = if line.is_empty() && !attachments.is_empty() {
        attachments
            .iter()
            .map(|file| file.name.as_str())
            .filter(|name| !name.is_empty())
            .take(3)
            .collect::<Vec<_>>()
            .join(", ")
    } else {
        String::new()
    };
    let seed = if line.is_empty() {
        from_files.as_str()
    } else {
        line
    };
    if seed.is_empty() {
        return harness.label().to_string();
    }
    const MAX: usize = 72;
    let short = if js::len(seed) > MAX {
        format!("{}…", js::slice_prefix(seed, MAX - 1))
    } else {
        seed.to_string()
    };
    format_session_title(harness, &short)
}

/// `formatSessionTitle`: "claude · Fix the build".
pub fn format_session_title(harness: HarnessId, title: &str) -> String {
    let trimmed = js::trim(title);
    if trimmed.is_empty() {
        return harness.label().to_string();
    }
    format!("{} · {trimmed}", harness.label())
}

/// `canReplaceSessionTitle`: the stored title is still a placeholder.
pub fn can_replace_session_title(current: &str, harness: HarnessId, seed: &str) -> bool {
    current == seed || current == harness.label() || current == harness.title()
}

/// `hasPendingApproval`.
pub fn has_pending_approval(blocks: &[Block]) -> bool {
    blocks.iter().any(|block| {
        block
            .approval
            .as_ref()
            .is_some_and(|approval| approval.decided.is_none())
    })
}

/// `sessionNeedsInput`.
pub fn session_needs_input(session: &Session) -> bool {
    session.worktree_removed != Some(true)
        && (has_pending_approval(&session.blocks) || session.pending_question.is_some())
}

/// `sessionDraftBlock`: the single unsent user turn, when present.
pub fn session_draft_block(blocks: &[Block]) -> Option<&Block> {
    blocks
        .iter()
        .find(|block| block.role == BlockRole::User && block.is_draft())
}

/// `removeSessionDraft`: remove one saved draft without disturbing the
/// conversation before it. `None` when `draft_block_id` is not a draft.
pub fn remove_session_draft(session: &Session, draft_block_id: &str) -> Option<Session> {
    let draft = session.blocks.iter().find(|block| {
        block.id == draft_block_id && block.role == BlockRole::User && block.is_draft()
    })?;
    let draft_title = title_from_prompt(
        &draft.text,
        session.harness,
        draft.attachments.as_deref().unwrap_or(&[]),
    );
    let mut next = session.clone();
    next.blocks.retain(|block| block.id != draft_block_id);
    if next.blocks.is_empty() && session.title == draft_title {
        next.title = session.harness.label().to_string();
    }
    Some(next)
}

/// `sessionDisplayTitle`: the title without the harness prefix.
pub fn session_display_title(title: &str, harness: HarnessId) -> String {
    let prefix = format!("{} · ", harness.label());
    if let Some(rest) = title.strip_prefix(&prefix) {
        return rest.to_string();
    }
    if title == harness.label() || title == harness.title() {
        return "New session".into();
    }
    title.to_string()
}

/// `sessionWorkCwd`: the working copy the agent and git views use.
pub fn session_work_cwd(session: &Session) -> &str {
    match session.worktree_cwd.as_deref() {
        Some(worktree) if !worktree.is_empty() => worktree,
        _ => &session.cwd,
    }
}

/// `USAGE_LIMIT_RESUME_GRACE_MS`: providers can still refuse right at the
/// reset, so give them a moment.
pub const USAGE_LIMIT_RESUME_GRACE_MS: i64 = 30_000;

/// `usageLimitResumeDue`: idle, armed, and past its reset.
pub fn usage_limit_resume_due(session: &Session, now: i64) -> bool {
    let Some(limit) = session.usage_limit else {
        return false;
    };
    let Some(resets_at) = limit.resets_at else {
        return false;
    };
    if limit.resume_at_reset != Some(true) || session.is_busy() {
        return false;
    }
    now >= resets_at + USAGE_LIMIT_RESUME_GRACE_MS
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{HarnessAvailability, ModelCatalog, ModelPrefs};
    use crate::project_providers::ProjectProviders;

    struct Fixture {
        catalog: ModelCatalog,
        prefs: ModelPrefs,
        availability: HarnessAvailability,
        projects: ProjectProviders,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                catalog: ModelCatalog::new(),
                prefs: ModelPrefs::default(),
                availability: HarnessAvailability::default(),
                projects: ProjectProviders::default(),
            }
        }

        fn env(&self) -> ModelEnv<'_> {
            ModelEnv {
                catalog: &self.catalog,
                prefs: &self.prefs,
                availability: &self.availability,
                projects: &self.projects,
            }
        }

        fn session(&self, harness: HarnessId, cwd: &str, model: Option<&str>) -> Session {
            new_session(&self.env(), "id", harness, cwd, model, None, None)
        }
    }

    fn user(id: &str, text: &str, draft: bool) -> Block {
        Block {
            draft: draft.then_some(true),
            ..Block::new(id, BlockRole::User, text)
        }
    }

    #[test]
    fn new_session_resolves_the_model_and_defaults() {
        let f = Fixture::new();
        let session = f.session(HarnessId::Grok, "/repo", None);
        assert_eq!(session.model, "grok:grok-4.6");
        assert_eq!(
            session.model_settings.get("effort").map(String::as_str),
            Some("high")
        );
        assert_eq!(session.title, "grok");
        assert_eq!(session.runtime_mode, RuntimeMode::Supervised);
        let json = serde_json::to_value(&session).unwrap();
        assert_eq!(json["runtimeMode"], "supervised");
        assert!(json.get("busy").is_none());
    }

    // live catalog overlays, the parts that go through newSession
    #[test]
    fn new_session_keeps_saved_claude_versions() {
        let mut f = Fixture::new();
        f.catalog.set_harness_models(
            HarnessId::Claude,
            vec![
                crate::models::AgentModel::new("claude:sonnet", HarnessId::Claude, "Sonnet")
                    .with_native_id("sonnet"),
                crate::models::AgentModel::new("claude:opus-5", HarnessId::Claude, "Opus 5")
                    .with_native_id("claude-opus-5"),
            ],
        );
        assert_eq!(
            f.session(HarnessId::Claude, "/repo", Some("claude:opus-5"))
                .model,
            "claude:opus-5"
        );
        assert_eq!(
            f.session(HarnessId::Claude, "/repo", Some("claude:opus-5-6"))
                .model,
            "claude:opus-5-6"
        );
        f.catalog.reset_overlays();
        assert_eq!(
            f.session(HarnessId::Claude, "/repo", Some("claude:opus"))
                .model,
            "claude:opus"
        );
    }

    // newSessionForProject
    #[test]
    fn keeps_a_seed_provider_the_project_allows() {
        let f = Fixture::new();
        let seed = f.session(HarnessId::Claude, "/repo/a", Some("claude:opus-5"));
        let session = new_session_for_project(&f.env(), "new", Some(&seed), "/repo/a");
        assert_eq!(session.harness, HarnessId::Claude);
        assert_eq!(session.model, "claude:opus-5");
    }

    #[test]
    fn falls_back_to_an_enabled_provider_when_the_seed_one_is_disabled() {
        let mut f = Fixture::new();
        f.projects
            .set_project_provider_hidden("/repo/a", HarnessId::Claude, true);
        let seed = f.session(HarnessId::Claude, "/repo/a", Some("claude:opus-5"));
        let session = new_session_for_project(&f.env(), "new", Some(&seed), "/repo/a");
        assert_eq!(session.harness, HarnessId::Codex);
        assert_eq!(session.cwd, "/repo/a");
    }

    #[test]
    fn does_not_carry_a_seed_model_across_providers() {
        let mut f = Fixture::new();
        f.projects
            .set_project_provider_hidden("/repo/a", HarnessId::Claude, true);
        let seed = f.session(HarnessId::Claude, "/repo/a", Some("claude:opus-5"));
        let session = new_session_for_project(&f.env(), "new", Some(&seed), "/repo/a");
        assert_ne!(session.model, "claude:opus-5");
    }

    #[test]
    fn uses_the_project_default_provider_even_when_the_seed_one_is_allowed() {
        let mut f = Fixture::new();
        f.projects.set_project_default_provider(
            "/repo/a",
            HarnessId::Cursor,
            "cursor:composer-2.5",
        );
        let seed = f.session(HarnessId::Claude, "/repo/a", Some("claude:opus-5"));
        let session = new_session_for_project(&f.env(), "new", Some(&seed), "/repo/a");
        assert_eq!(session.harness, HarnessId::Cursor);
        assert_eq!(session.model, "cursor:composer-2.5");
    }

    #[test]
    fn applies_a_project_model_override_to_the_carried_provider() {
        let mut f = Fixture::new();
        f.projects
            .set_project_default_model("/repo/a", HarnessId::Claude, "claude:haiku-4.5");
        let seed = f.session(HarnessId::Claude, "/repo/a", Some("claude:opus-5"));
        let session = new_session_for_project(&f.env(), "new", Some(&seed), "/repo/a");
        assert_eq!(session.harness, HarnessId::Claude);
        assert_eq!(session.model, "claude:haiku-4.5");
    }

    // retargetSessionToProject
    #[test]
    fn adopts_the_project_defaults_while_keeping_the_session_identity() {
        let mut f = Fixture::new();
        f.projects.set_project_default_provider(
            "/repo/a",
            HarnessId::Cursor,
            "cursor:composer-2.5",
        );
        let blank = f.session(HarnessId::Claude, "~", Some("claude:opus-5"));
        let retargeted = retarget_session_to_project(&f.env(), &blank, "/repo/a");
        assert_eq!(retargeted.id, blank.id);
        assert_eq!(retargeted.cwd, "/repo/a");
        assert_eq!(retargeted.harness, HarnessId::Cursor);
        assert_eq!(retargeted.model, "cursor:composer-2.5");
    }

    #[test]
    fn keeps_the_sessions_provider_when_the_project_has_no_defaults() {
        let f = Fixture::new();
        let blank = f.session(HarnessId::Claude, "~", Some("claude:opus-5"));
        let retargeted = retarget_session_to_project(&f.env(), &blank, "/repo/a");
        assert_eq!(retargeted.harness, HarnessId::Claude);
        assert_eq!(retargeted.model, "claude:opus-5");
    }

    #[test]
    fn drops_provider_bound_fields_when_retargeting_changes_the_harness() {
        let mut f = Fixture::new();
        f.projects.set_project_default_provider(
            "/repo/a",
            HarnessId::Cursor,
            "cursor:composer-2.5",
        );
        let mut blank = f.session(HarnessId::Claude, "~", Some("claude:opus-5"));
        blank.provider_session_id = Some("claude-session".into());
        blank.provider_account_id = Some("claude-account".into());
        blank.context = Some(ContextUsage {
            used: 10,
            window: Some(100),
        });
        let retargeted = retarget_session_to_project(&f.env(), &blank, "/repo/a");
        assert_eq!(retargeted.harness, HarnessId::Cursor);
        assert_eq!(retargeted.provider_session_id, None);
        assert_eq!(retargeted.provider_account_id, None);
        assert_eq!(
            retargeted.context,
            Some(ContextUsage {
                used: 10,
                window: None
            })
        );
    }

    #[test]
    fn keeps_provider_bound_fields_when_the_harness_is_unchanged() {
        let mut f = Fixture::new();
        f.projects
            .set_project_default_model("/repo/a", HarnessId::Claude, "claude:haiku-4.5");
        let mut blank = f.session(HarnessId::Claude, "~", Some("claude:opus-5"));
        blank.provider_session_id = Some("claude-session".into());
        blank.provider_account_id = Some("claude-account".into());
        let retargeted = retarget_session_to_project(&f.env(), &blank, "/repo/a");
        assert_eq!(retargeted.harness, HarnessId::Claude);
        assert_eq!(
            retargeted.provider_session_id.as_deref(),
            Some("claude-session")
        );
        assert_eq!(
            retargeted.provider_account_id.as_deref(),
            Some("claude-account")
        );
    }

    // removeSessionDraft
    #[test]
    fn removes_a_follow_up_draft_without_changing_earlier_conversation_history() {
        let f = Fixture::new();
        let mut session = f.session(HarnessId::Codex, "/repo", None);
        session.title = "codex · Existing thread".into();
        session.blocks = vec![
            user("sent", "Start here", false),
            Block::new("reply", BlockRole::Assistant, "Done"),
            user("draft", "Maybe later", true),
        ];
        let updated = remove_session_draft(&session, "draft").unwrap();
        assert_eq!(updated.blocks, session.blocks[..2]);
        assert_eq!(updated.title, "codex · Existing thread");
    }

    #[test]
    fn restores_a_draft_only_session_to_a_blank_untitled_state() {
        let f = Fixture::new();
        let mut session = f.session(HarnessId::Codex, "/repo", None);
        session.title = "codex · Maybe later".into();
        session.blocks = vec![user("draft", "Maybe later", true)];
        let updated = remove_session_draft(&session, "draft").unwrap();
        assert_eq!(updated.title, "codex");
        assert!(updated.blocks.is_empty());
    }

    #[test]
    fn keeps_a_custom_title_when_removing_the_only_draft() {
        let f = Fixture::new();
        let mut session = f.session(HarnessId::Codex, "/repo", None);
        session.title = "codex · Keep this name".into();
        session.blocks = vec![user("draft", "Maybe later", true)];
        assert_eq!(
            remove_session_draft(&session, "draft").unwrap().title,
            "codex · Keep this name"
        );
    }

    #[test]
    fn ignores_sent_messages_and_unknown_blocks() {
        let f = Fixture::new();
        let mut session = f.session(HarnessId::Codex, "/repo", None);
        session.blocks = vec![user("sent", "Keep this", false)];
        assert!(remove_session_draft(&session, "sent").is_none());
        assert!(remove_session_draft(&session, "missing").is_none());
    }

    // usageLimitResumeDue
    fn limited(f: &Fixture) -> Session {
        let mut session = f.session(HarnessId::Codex, "/tmp/project", None);
        session.usage_limit = Some(UsageLimit {
            resets_at: Some(10_000),
            resume_at_reset: Some(true),
        });
        session
    }

    #[test]
    fn waits_for_the_reset_plus_a_grace_period() {
        let f = Fixture::new();
        assert!(!usage_limit_resume_due(&limited(&f), 10_000));
        assert!(usage_limit_resume_due(
            &limited(&f),
            10_000 + USAGE_LIMIT_RESUME_GRACE_MS
        ));
    }

    #[test]
    fn only_resumes_idle_sessions_the_user_armed() {
        let f = Fixture::new();
        let later = 10_000 + USAGE_LIMIT_RESUME_GRACE_MS;
        let mut busy = limited(&f);
        busy.busy = Some(true);
        assert!(!usage_limit_resume_due(&busy, later));
        let mut unarmed = limited(&f);
        unarmed.usage_limit = Some(UsageLimit {
            resets_at: Some(10_000),
            resume_at_reset: None,
        });
        assert!(!usage_limit_resume_due(&unarmed, later));
        let mut unknown = limited(&f);
        unknown.usage_limit = Some(UsageLimit {
            resets_at: None,
            resume_at_reset: Some(true),
        });
        assert!(!usage_limit_resume_due(&unknown, later));
    }

    #[test]
    fn titles_follow_the_first_prompt_line() {
        let file = Attachment {
            name: "shot.png".into(),
            ..Attachment::default()
        };
        assert_eq!(
            title_from_prompt("  Fix the build\nmore", HarnessId::Claude, &[]),
            "claude · Fix the build"
        );
        assert_eq!(
            title_from_prompt("", HarnessId::Claude, &[file]),
            "claude · shot.png"
        );
        assert_eq!(title_from_prompt(" ", HarnessId::Codex, &[]), "codex");
        let long = "x".repeat(80);
        let title = title_from_prompt(&long, HarnessId::Pi, &[]);
        assert_eq!(title, format!("pi · {}…", "x".repeat(71)));
        assert_eq!(
            session_display_title("claude · Fix", HarnessId::Claude),
            "Fix"
        );
        assert_eq!(
            session_display_title("Claude Code", HarnessId::Claude),
            "New session"
        );
        assert_eq!(session_display_title("Other", HarnessId::Claude), "Other");
        assert!(can_replace_session_title(
            "Claude Code",
            HarnessId::Claude,
            "seed"
        ));
        assert!(!can_replace_session_title(
            "claude · Real",
            HarnessId::Claude,
            "seed"
        ));
    }

    #[test]
    fn needs_input_for_pending_approvals_and_questions() {
        let f = Fixture::new();
        let mut session = f.session(HarnessId::Claude, "/repo", None);
        assert!(!session_needs_input(&session));
        session.blocks.push(Block {
            approval: Some(crate::block::BlockApproval {
                request_id: 1,
                decided: None,
                extra: Extra::new(),
            }),
            ..Block::new("a", BlockRole::Approval, "Run?")
        });
        assert!(session_needs_input(&session));
        session.worktree_removed = Some(true);
        assert!(!session_needs_input(&session));
        assert_eq!(session_work_cwd(&session), "/repo");
        session.worktree_cwd = Some("/wt".into());
        assert_eq!(session_work_cwd(&session), "/wt");
        assert!(session_draft_block(&session.blocks).is_none());
    }
}
