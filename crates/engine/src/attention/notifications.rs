//! Port of the pure parts of src/features/notifications/model/notifications.ts:
//! the opt-in setting, the banner policy, pending input requests, and the
//! banner text. The permission cache, the window focus flag, and delivery
//! (`notifySession`, `announceSessionFinished`) live on the `Notifier`
//! entity.

use std::sync::LazyLock;

use monocode_core::block::BlockRole;
use monocode_core::js;
use monocode_core::session::{Session, session_display_title};
use monocode_settings::Kv;
use regex::Regex;
use serde::{Deserialize, Serialize};

/// The opt-in key.
pub const NOTIFICATIONS_KEY: &str = "monocode.notifications";

/// `NOTIFICATIONS_DEFAULT`: off until the user opts in; enabling asks the OS
/// for permission.
pub const NOTIFICATIONS_DEFAULT: bool = false;

/// `NotificationPermission`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NotificationPermission {
    #[serde(rename = "prompt")]
    Prompt,
    #[serde(rename = "granted")]
    Granted,
    #[serde(rename = "denied")]
    Denied,
    #[serde(rename = "unsupported")]
    Unsupported,
}

impl From<monocode_platform::notifications::Permission> for NotificationPermission {
    fn from(permission: monocode_platform::notifications::Permission) -> Self {
        use monocode_platform::notifications::Permission;
        match permission {
            Permission::Prompt => NotificationPermission::Prompt,
            Permission::Granted => NotificationPermission::Granted,
            Permission::Denied => NotificationPermission::Denied,
            Permission::Unsupported => NotificationPermission::Unsupported,
        }
    }
}

/// `"1"` or `"true"`.
pub(crate) fn stored_flag(raw: Option<String>, default: bool) -> bool {
    match raw {
        None => default,
        Some(raw) => raw == "1" || raw == "true",
    }
}

/// `loadNotificationsEnabled`.
pub fn load_notifications_enabled(kv: &Kv) -> bool {
    stored_flag(kv.get_item(NOTIFICATIONS_KEY), NOTIFICATIONS_DEFAULT)
}

/// `saveNotificationsEnabled`. The `Kv` change replaces
/// `NOTIFICATIONS_CHANGE_EVENT`.
pub fn save_notifications_enabled(kv: &Kv, value: bool) {
    kv.set_item(NOTIFICATIONS_KEY, if value { "1" } else { "0" });
}

/// `shouldNotify`: a banner only earns its place while the user is looking
/// elsewhere, at another app or another session. The transcript already
/// shows the change on the session that is on screen.
pub fn should_notify(
    enabled: bool,
    permission: NotificationPermission,
    window_focused: bool,
    session_visible: bool,
) -> bool {
    if !enabled || (window_focused && session_visible) {
        return false;
    }
    matches!(
        permission,
        NotificationPermission::Granted | NotificationPermission::Prompt
    )
}

/// `InputNotificationEvent["kind"]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InputKind {
    #[serde(rename = "approval")]
    Approval,
    #[serde(rename = "question")]
    Question,
}

impl InputKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            InputKind::Approval => "approval",
            InputKind::Question => "question",
        }
    }
}

/// `NotificationEvent`: a finished turn, or an approval or question.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NotificationEvent {
    Finished,
    Input { kind: InputKind, request_id: i64 },
}

/// One entry of `pendingInputNotifications`.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingInputNotification {
    /// `JSON.stringify([sessionId, kind, requestId])`.
    pub key: String,
    pub session_id: String,
    pub kind: InputKind,
    pub request_id: i64,
}

fn input_key(session_id: &str, kind: InputKind, request_id: i64) -> String {
    serde_json::json!([session_id, kind.as_str(), request_id]).to_string()
}

/// `pendingInputNotifications`: every open request, including a new request
/// in an already-waiting session, in insertion order. A repeated key keeps
/// its first position, as a `Map` does.
pub fn pending_input_notifications(sessions: &[Session]) -> Vec<PendingInputNotification> {
    let mut pending: Vec<PendingInputNotification> = Vec::new();
    let mut push = |entry: PendingInputNotification| {
        if !pending.iter().any(|existing| existing.key == entry.key) {
            pending.push(entry);
        }
    };
    for session in sessions {
        if session.inbox_ask.is_some() {
            continue;
        }
        for block in &session.blocks {
            if let Some(approval) = block
                .approval
                .as_ref()
                .filter(|approval| approval.decided.is_none())
            {
                push(PendingInputNotification {
                    key: input_key(&session.id, InputKind::Approval, approval.request_id),
                    session_id: session.id.clone(),
                    kind: InputKind::Approval,
                    request_id: approval.request_id,
                });
            }
        }
        if let Some(question) = &session.pending_question {
            push(PendingInputNotification {
                key: input_key(&session.id, InputKind::Question, question.request_id),
                session_id: session.id.clone(),
                kind: InputKind::Question,
                request_id: question.request_id,
            });
        }
    }
    pending
}

/// `NotificationText`: the app name, then the session title, then the reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationText {
    pub title: String,
    pub subtitle: String,
    pub body: String,
}

const BODY_MAX: usize = 240;

/// JavaScript `\s` as a regex class.
const JS_SPACE: &str = r"[\t\n\x0B\x0C\r \u{a0}\u{1680}\u{2000}-\u{200a}\u{2028}\u{2029}\u{202f}\u{205f}\u{3000}\u{feff}]";

static PARAGRAPH_BREAK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"\n{JS_SPACE}*\n")).expect("valid regex"));
static SPACE_RUN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!("{JS_SPACE}+")).expect("valid regex"));

/// `notificationText`.
pub fn notification_text(session: &Session, event: NotificationEvent) -> NotificationText {
    let title = "MonoCode".to_string();
    let subtitle = session_display_title(&session.title, session.harness);
    let harness = session.harness.title();
    if let NotificationEvent::Input { kind, request_id } = event {
        if kind == InputKind::Question {
            let question = session
                .pending_question
                .as_ref()
                .filter(|question| question.request_id == request_id);
            let prompt = question.and_then(|question| {
                question
                    .title
                    .clone()
                    .filter(|title| !title.is_empty())
                    .or_else(|| question.questions.first().map(|first| first.prompt.clone()))
            });
            let body = prompt
                .filter(|prompt| !prompt.is_empty())
                .unwrap_or_else(|| format!("{harness} has a question for you"));
            return NotificationText {
                title,
                subtitle,
                body: clip(&body),
            };
        }
        let pending = session.blocks.iter().find(|block| {
            block.approval.as_ref().is_some_and(|approval| {
                approval.request_id == request_id && approval.decided.is_none()
            })
        });
        let what = pending.and_then(|block| {
            block
                .tool
                .as_ref()
                .and_then(|tool| tool.title.clone())
                .filter(|title| !title.is_empty())
                .or_else(|| (!block.text.is_empty()).then(|| block.text.clone()))
        });
        let body = match what {
            Some(what) => format!("Approve: {what}"),
            None => format!("{harness} needs your approval"),
        };
        return NotificationText {
            title,
            subtitle,
            body: clip(&body),
        };
    }
    let reply = session
        .blocks
        .iter()
        .rev()
        .find(|block| block.role == BlockRole::Assistant && !js::trim(&block.text).is_empty());
    let body = match reply {
        Some(reply) => reply.text.clone(),
        None => format!("{harness} finished"),
    };
    NotificationText {
        title,
        subtitle,
        body: clip(&body),
    }
}

/// `clip`: the first paragraph with whitespace collapsed; macOS wraps and
/// truncates the rest.
fn clip(text: &str) -> String {
    let paragraph = PARAGRAPH_BREAK
        .split(text)
        .map(|part| js::trim(&SPACE_RUN.replace_all(part, " ")).to_string())
        .find(|part| !part.is_empty())
        .unwrap_or_default();
    if js::len(&paragraph) > BODY_MAX {
        format!("{}…", js::slice_prefix(&paragraph, BODY_MAX - 1))
    } else {
        paragraph
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use monocode_core::block::{ApprovalDecided, Block, BlockApproval, BlockTool};
    use monocode_core::user_question::{UserQuestion, UserQuestionPrompt};
    use monocode_core::{Extra, HarnessId};

    pub(crate) fn approval_block(id: &str, role: BlockRole, text: &str, request_id: i64) -> Block {
        let mut block = Block::new(id, role, text);
        block.approval = Some(BlockApproval {
            request_id,
            decided: None,
            extra: Extra::new(),
        });
        block
    }

    pub(crate) fn question(
        request_id: i64,
        title: Option<&str>,
        prompts: &[&str],
    ) -> UserQuestionPrompt {
        UserQuestionPrompt {
            request_id,
            title: title.map(str::to_string),
            questions: prompts
                .iter()
                .enumerate()
                .map(|(index, prompt)| UserQuestion {
                    id: format!("q{index}"),
                    header: None,
                    prompt: prompt.to_string(),
                    multi_select: false,
                    allow_custom: false,
                    options: Vec::new(),
                })
                .collect(),
            auto_resolve_at: None,
        }
    }

    fn chat(blocks: Vec<Block>) -> Session {
        let mut session = Session::blank("s1", HarnessId::Claude, "sonnet", "/tmp/a");
        session.title = "claude · Fix the sidebar".into();
        session.blocks = if blocks.is_empty() {
            vec![Block::new("u1", BlockRole::User, "hello")]
        } else {
            blocks
        };
        session
    }

    fn keys(sessions: &[Session]) -> Vec<String> {
        pending_input_notifications(sessions)
            .into_iter()
            .map(|entry| entry.key)
            .collect()
    }

    #[test]
    fn detects_a_second_approval_without_an_intervening_idle_render() {
        let session = chat(vec![approval_block("p1", BlockRole::Tool, "first", 1)]);
        let before = keys(std::slice::from_ref(&session));
        let mut next = session.clone();
        next.blocks
            .push(approval_block("p2", BlockRole::Tool, "second", 2));
        let after = keys(std::slice::from_ref(&next));
        assert_eq!(after.iter().filter(|key| !before.contains(key)).count(), 1);
        // An ordinary transcript update must not repeat either notification.
        let mut retitled = next.clone();
        retitled.title = "changed".into();
        assert_eq!(keys(&[retitled]), after);
        let mut resolved = next.clone();
        for block in &mut resolved.blocks {
            if let Some(approval) = block.approval.as_mut() {
                approval.decided = Some(ApprovalDecided::Allow);
            }
        }
        assert!(pending_input_notifications(&[resolved]).is_empty());
    }

    #[test]
    fn keeps_questions_and_approvals_in_different_sessions_distinct() {
        let mut first = chat(vec![approval_block("p1", BlockRole::Tool, "approve", 1)]);
        first.pending_question = Some(question(1, None, &[]));
        let mut second = first.clone();
        second.id = "other".into();
        assert_eq!(
            pending_input_notifications(&[first.clone(), second]).len(),
            4
        );
        let before = keys(std::slice::from_ref(&first));
        let mut asked = first.clone();
        asked.pending_question = Some(question(2, None, &[]));
        assert_eq!(
            keys(&[asked])
                .iter()
                .filter(|key| !before.contains(key))
                .count(),
            1
        );
    }

    #[test]
    fn the_setting_is_off_until_the_user_opts_in_and_round_trips() {
        let kv = Kv::in_memory();
        const { assert!(!NOTIFICATIONS_DEFAULT) };
        assert!(!load_notifications_enabled(&kv));
        save_notifications_enabled(&kv, true);
        assert!(load_notifications_enabled(&kv));
        save_notifications_enabled(&kv, false);
        assert!(!load_notifications_enabled(&kv));
    }

    #[test]
    fn stays_quiet_while_the_session_is_on_screen_in_a_focused_window() {
        assert!(!should_notify(
            true,
            NotificationPermission::Granted,
            true,
            true
        ));
    }

    #[test]
    fn fires_for_a_session_that_is_not_on_screen_even_when_focused() {
        assert!(should_notify(
            true,
            NotificationPermission::Granted,
            true,
            false
        ));
    }

    #[test]
    fn respects_the_toggle_and_the_os_decision() {
        assert!(!should_notify(
            false,
            NotificationPermission::Granted,
            false,
            true
        ));
        assert!(!should_notify(
            true,
            NotificationPermission::Denied,
            false,
            true
        ));
        assert!(!should_notify(
            true,
            NotificationPermission::Unsupported,
            false,
            true
        ));
    }

    #[test]
    fn fires_when_unfocused_and_allowed_or_still_undecided() {
        assert!(should_notify(
            true,
            NotificationPermission::Granted,
            false,
            true
        ));
        assert!(should_notify(
            true,
            NotificationPermission::Prompt,
            false,
            true
        ));
    }

    #[test]
    fn leads_with_the_app_then_the_session_title_then_the_reply() {
        let session = chat(vec![
            Block::new("u1", BlockRole::User, "hello"),
            Block::new(
                "a1",
                BlockRole::Assistant,
                "\n\nDone. Sidebar\nfixed.\n\nDetails below.",
            ),
        ]);
        assert_eq!(
            notification_text(&session, NotificationEvent::Finished),
            NotificationText {
                title: "MonoCode".into(),
                subtitle: "Fix the sidebar".into(),
                body: "Done. Sidebar fixed.".into(),
            }
        );
    }

    #[test]
    fn falls_back_to_a_generic_body_without_a_reply() {
        assert_eq!(
            notification_text(&chat(vec![]), NotificationEvent::Finished).body,
            "Claude Code finished"
        );
    }

    #[test]
    fn clips_long_bodies() {
        let session = chat(vec![Block::new(
            "a1",
            BlockRole::Assistant,
            "x".repeat(400),
        )]);
        let body = notification_text(&session, NotificationEvent::Finished).body;
        assert_eq!(js::len(&body), 240);
        assert!(body.ends_with('…'));
    }

    #[test]
    fn names_the_pending_approval() {
        let mut block = approval_block("p1", BlockRole::Approval, "Run npm test", 1);
        block.tool = Some(BlockTool {
            title: Some("Run npm test".into()),
            ..Default::default()
        });
        let session = chat(vec![block]);
        assert_eq!(
            notification_text(
                &session,
                NotificationEvent::Input {
                    kind: InputKind::Approval,
                    request_id: 1
                }
            ),
            NotificationText {
                title: "MonoCode".into(),
                subtitle: "Fix the sidebar".into(),
                body: "Approve: Run npm test".into(),
            }
        );
    }

    #[test]
    fn names_the_requested_question() {
        let mut session = chat(vec![]);
        session.pending_question = Some(question(2, None, &["Which database?"]));
        assert_eq!(
            notification_text(
                &session,
                NotificationEvent::Input {
                    kind: InputKind::Question,
                    request_id: 2
                }
            ),
            NotificationText {
                title: "MonoCode".into(),
                subtitle: "Fix the sidebar".into(),
                body: "Which database?".into(),
            }
        );
    }
}
