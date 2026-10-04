//! Ported Codex tests.
//!
//! - `protocol`: codexProtocol.test.ts.
//! - `session`, `subagents`: codexLive.test.ts, which mocks the child, over
//!   the scripted app-server in `fake`.
//! - `approval_ui`, `attachments`: the protocol side of codexApprovalUi.test.ts
//!   and codexAttachments.test.ts.
//! - `live`: one `#[ignore]` turn against the real `codex` CLI.
//! - `restore`: the "Codex Shell row recovery" cases of sessionStore.test.ts,
//!   since the repair lives next to the protocol it must agree with.
//!
//! codexElicitation.test.ts and codexQuestions.test.ts sit next to their
//! modules.

mod approval_ui;
mod attachments;
mod fake;
mod live;
mod protocol;
mod restore;
mod session;
mod subagents;
mod support;
mod text;
