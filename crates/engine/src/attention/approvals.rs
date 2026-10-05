//! The `Approvals` entity: pending approvals and questions across sessions,
//! the answers, and the provider sign-in prompt.
//!
//! Ports App.tsx `approvalSessionIds` (lines 1631-1645), the provider
//! sign-in request effects (lines 1576-1607 and 1016-1030), `onApproval`,
//! `onQuestionReply`, `onQuestionInteraction`, and `onOpenApprovalSession`
//! (lines 8564-8614), plus `hiddenApprovalNotices` over the live sessions.
//! The answers go through the `ApprovalRouter` hook.

use std::collections::HashSet;

use gpui::{App, Context, Entity, EventEmitter, Subscription};
use monocode_core::HarnessId;
use monocode_core::block::BlockRole;
use monocode_core::harness_event::ApprovalDecision;
use monocode_core::session::{Session, session_needs_input};
use monocode_core::user_question::UserQuestionReply;
use monocode_harness::core::auth_support::{
    latest_turn_needs_harness_login, supports_harness_login,
};
use monocode_layout::WorkspaceTab;

use super::approval_toast::{
    PendingApprovalNotice, hidden_approval_notices, pending_approval_for_session,
};
use super::{Attention, AttentionFocus};
use crate::runtime::engine::Engine;
use crate::runtime::sessions::Sessions;

/// The provider sign-in dialog request (`providerSignInRequest`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSignInRequest {
    pub key: String,
    pub session_id: String,
    pub harness: HarnessId,
}

/// What changed on `Approvals`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalsEvent {
    /// The set of sessions waiting on the user changed.
    Changed,
    /// Open the provider sign-in dialog.
    SignInRequested(ProviderSignInRequest),
    /// Close the provider sign-in dialog.
    SignInClosed,
}

/// `providerSignInRequestKey`: one prompt per failed turn.
pub fn provider_sign_in_request_key(session: &Session) -> String {
    let last_user = session
        .blocks
        .iter()
        .rev()
        .find(|block| block.role == BlockRole::User)
        .map(|block| block.id.as_str());
    let last_block = session.blocks.last().map(|block| block.id.as_str());
    format!(
        "{}:{}",
        session.id,
        last_user.or(last_block).unwrap_or("auth")
    )
}

/// `activeProviderSignInRequest`.
pub fn provider_sign_in_request(session: &Session) -> Option<ProviderSignInRequest> {
    if !supports_harness_login(session.harness) || !latest_turn_needs_harness_login(&session.blocks)
    {
        return None;
    }
    Some(ProviderSignInRequest {
        key: provider_sign_in_request_key(session),
        session_id: session.id.clone(),
        harness: session.harness,
    })
}

/// `approvalSessionIds`: sessions waiting on the user, with the leads of
/// waiting workers.
pub fn approval_session_ids(sessions: &[Session]) -> HashSet<String> {
    let mut ids = HashSet::new();
    for session in sessions {
        if session_needs_input(session) {
            ids.insert(session.id.clone());
            if let Some(lead) = &session.orchestration_lead_id {
                ids.insert(lead.clone());
            }
        }
    }
    ids
}

/// Pending approvals and questions, and the sign-in prompt.
pub struct Approvals {
    approval_session_ids: HashSet<String>,
    focus: AttentionFocus,
    sign_in: Option<ProviderSignInRequest>,
    /// `seenProviderSignInRequests`: seeded with the sessions that already
    /// needed a sign-in at launch, so a restored failure does not prompt.
    seen_sign_in: HashSet<String>,
    _observe: Subscription,
}

impl EventEmitter<ApprovalsEvent> for Approvals {}

impl Approvals {
    pub fn new(sessions: &Entity<Sessions>, cx: &mut Context<Self>) -> Self {
        let all = sessions.read(cx).all();
        let seen_sign_in = all
            .iter()
            .filter_map(provider_sign_in_request)
            .map(|request| request.key)
            .collect();
        let approval_session_ids = approval_session_ids(all);
        Self {
            approval_session_ids,
            focus: AttentionFocus::default(),
            sign_in: None,
            seen_sign_in,
            _observe: cx.observe(sessions, |this, _, cx| this.sessions_changed(cx)),
        }
    }

    // Reading.

    /// Sessions waiting on the user, with the leads of waiting workers.
    pub fn approval_session_ids(&self) -> &HashSet<String> {
        &self.approval_session_ids
    }

    /// The open sign-in prompt.
    pub fn sign_in_request(&self) -> Option<&ProviderSignInRequest> {
        self.sign_in.as_ref()
    }

    /// `pendingApprovalForSession` for an open session.
    pub fn pending_for(&self, session_id: &str, cx: &App) -> Option<PendingApprovalNotice> {
        let sessions = Engine::sessions(cx);
        pending_approval_for_session(sessions.read(cx).get(session_id)?)
    }

    /// `hiddenApprovalToasts`: pending requests outside the focused
    /// conversation. The workspace passes its tabs and composer focus.
    pub fn hidden_notices(
        &self,
        active_tab_id: &str,
        tabs: &[WorkspaceTab],
        composer_focused: bool,
        cx: &App,
    ) -> Vec<PendingApprovalNotice> {
        let sessions = Engine::sessions(cx);
        hidden_approval_notices(
            sessions.read(cx).all(),
            active_tab_id,
            tabs,
            composer_focused,
        )
    }

    // Changes.

    pub fn set_focus(&mut self, focus: AttentionFocus, cx: &mut Context<Self>) {
        if self.focus == focus {
            return;
        }
        self.focus = focus;
        self.sync_sign_in(cx);
    }

    /// The sign-in dialog closed.
    pub fn dismiss_sign_in(&mut self, cx: &mut Context<Self>) {
        if self.sign_in.take().is_some() {
            cx.emit(ApprovalsEvent::SignInClosed);
            cx.notify();
        }
    }

    fn sessions_changed(&mut self, cx: &mut Context<Self>) {
        let ids = approval_session_ids(Engine::sessions(cx).read(cx).all());
        if ids != self.approval_session_ids {
            self.approval_session_ids = ids;
            cx.emit(ApprovalsEvent::Changed);
            cx.notify();
        }
        self.sync_sign_in(cx);
    }

    /// The two sign-in effects: prompt once per failed turn of the active
    /// session, and close the prompt when another session becomes active.
    fn sync_sign_in(&mut self, cx: &mut Context<Self>) {
        let active_id = self.focus.active_session_id.clone();
        let request = active_id.as_deref().and_then(|id| {
            Engine::sessions(cx)
                .read(cx)
                .get(id)
                .and_then(provider_sign_in_request)
        });
        if let Some(request) = request
            && self.seen_sign_in.insert(request.key.clone())
        {
            self.sign_in = Some(request.clone());
            cx.emit(ApprovalsEvent::SignInRequested(request));
            cx.notify();
        }
        if self
            .sign_in
            .as_ref()
            .is_some_and(|request| active_id.as_deref() != Some(request.session_id.as_str()))
        {
            self.dismiss_sign_in(cx);
        }
    }

    // Answers. These read `Sessions` and call the router, so they take the
    // app rather than the entity.

    fn session(session_id: &str, cx: &App) -> Option<Session> {
        Engine::sessions(cx).read(cx).get(session_id).cloned()
    }

    /// `onApproval`.
    pub fn approve(session_id: &str, request_id: i64, decision: ApprovalDecision, cx: &mut App) {
        let Some(session) =
            Self::session(session_id, cx).filter(|session| session.worktree_removed != Some(true))
        else {
            return;
        };
        let router = Attention::router(cx);
        if router.is_remote(&session, cx) {
            router.remote_approve(session_id, request_id, decision, cx);
            return;
        }
        router.respond_approval(session.harness, session_id, request_id, decision, cx);
    }

    /// `onQuestionReply`.
    pub fn answer_question(
        session_id: &str,
        request_id: i64,
        reply: &UserQuestionReply,
        cx: &mut App,
    ) {
        let Some(session) =
            Self::session(session_id, cx).filter(|session| session.worktree_removed != Some(true))
        else {
            return;
        };
        let router = Attention::router(cx);
        if router.is_remote(&session, cx) {
            router.remote_answer(session_id, request_id, reply, cx);
            return;
        }
        router.respond_question(session.harness, session_id, request_id, reply, cx);
    }

    /// `onQuestionInteraction`: the user started answering.
    pub fn question_interaction(session_id: &str, request_id: i64, cx: &mut App) {
        let Some(session) = Self::session(session_id, cx) else {
            return;
        };
        let router = Attention::router(cx);
        if router.is_remote(&session, cx) {
            return;
        }
        if session.worktree_removed != Some(true) {
            router.keep_question_open(session.harness, session_id, request_id, cx);
        }
    }

    /// `onOpenApprovalSession`: a worker opens inside its lead's panel;
    /// anything else opens its own tab, from history when it is not open.
    pub fn open_approval_session(session_id: &str, cx: &mut App) {
        let router = Attention::router(cx);
        let parent_id = Self::session(session_id, cx)
            .and_then(|session| session.orchestration_lead_id)
            .or_else(|| router.orchestration_lead_for(session_id, cx));
        match parent_id.filter(|parent| parent != session_id) {
            Some(parent) => {
                router.inspect_worker(session_id, cx);
                if !router.focus_open_session(&parent, cx) {
                    router.open_history_session(&parent, cx);
                }
            }
            None => {
                if !router.focus_open_session(session_id, cx) {
                    router.open_history_session(session_id, cx);
                }
            }
        }
    }
}
