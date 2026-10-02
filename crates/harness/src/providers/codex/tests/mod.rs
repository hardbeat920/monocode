//! Ported Codex tests.
//!
//! - `protocol`: codexProtocol.test.ts.
//! - `session`, `subagents`: codexLive.test.ts, which mocks the child, over
//!   the scripted app-server in `fake`.
//! - `approval_ui`, `attachments`: the protocol side of codexApprovalUi.test.ts
//!   and codexAttachments.test.ts.
//! - `live`: one `#[ignore]` turn against the real `codex` CLI.
//!
//! codexElicitation.test.ts and codexQuestions.test.ts sit next to their
//! modules.

mod approval_ui;
mod attachments;
mod fake;
mod live;
mod protocol;
mod session;
mod subagents;
mod support;
mod text;
