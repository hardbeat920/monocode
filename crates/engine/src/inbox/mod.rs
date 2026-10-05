//! Engine package `inbox`: GitHub, GitLab, Azure DevOps, Jira, and Linear
//! work items, ported from src/features/inbox/model, src/features/inbox/hooks,
//! src/features/sessions/model/sessionWorkItem.ts, and the inbox parts of
//! src/app/App.tsx.

pub mod azure_devops;
pub mod backend;
pub mod ci_repair;
pub mod ci_repair_sessions;
pub mod ci_repair_tracking;
pub mod client;
pub mod detail;
pub mod github_pr_checks;
pub mod github_tasks;
pub mod gitlab;
pub mod hooks;
#[allow(clippy::module_inception)]
pub mod inbox;
pub mod inbox_ask;
pub mod inbox_context;
pub mod inbox_filters;
pub mod inbox_media;
pub mod inbox_notifications;
pub mod inbox_seen;
pub mod inbox_self_activity;
pub mod jira;
pub mod linear;
pub mod linked_session_seen;
pub mod linked_session_updates;
pub mod linked_work_item_activity;
pub mod list;
pub mod pr_checks;
pub mod rail;
pub mod session_work_item;
pub mod text;
pub mod time;
pub mod types;

#[cfg(test)]
mod client_tests;
#[cfg(test)]
mod inbox_tests;
#[cfg(test)]
mod list_detail_tests;
#[cfg(test)]
mod pr_checks_tests;
