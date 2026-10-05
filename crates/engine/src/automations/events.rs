//! Port of src/features/automations/model/automationEvents.ts: Inbox items
//! that appear start the automations whose triggers match them. Each
//! automation claims an item once through the store's event ledger; items
//! whose claim failed wait in a retry list saved under
//! `monocode.automation-inbox-retries.v1`.
//!
//! The Inbox item type lives in the unfinished inbox package, so this module
//! reads the same JSON shape through `InboxEventItem` and carries local
//! copies of `inboxStartDraft`, `linkedWorkItemFromInboxItem`, and
//! `linkedWorkItemFromAutomationEvent` (NEEDS.md).

use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::LazyLock;

use chrono::{DateTime, Local, LocalResult, NaiveDate, NaiveDateTime, TimeZone};
use monocode_core::Extra;
use monocode_core::inbox::{InboxKind, InboxProvider, WorkItemKind};
use monocode_core::js;
use monocode_core::session::LinkedWorkItem;
use monocode_settings::Kv;
use regex::Regex;
use serde::{Deserialize, Serialize};

use super::backend::AutomationsBackend;
use super::model::{
    Automation, AutomationRun, AutomationRunTrigger, AutomationTrigger, AutomationTriggerKind,
    DueAutomationRun, automation_triggers,
};
use crate::runtime::util::project_path::same_project_path;

/// The retry list's key.
pub const RETRY_STORAGE_KEY: &str = "monocode.automation-inbox-retries.v1";
/// `MAX_RETRY_ITEMS`.
pub const MAX_RETRY_ITEMS: usize = 500;

/// The fields of an Inbox `InboxItem` that triggers read. Other fields
/// survive in `extra`, so the retry list saves whole items.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxEventItem {
    pub kind: InboxKind,
    pub number: i64,
    pub title: String,
    pub url: String,
    #[serde(default)]
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub repo: String,
    #[serde(default)]
    pub project_path: String,
    pub provider: InboxProvider,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identifier: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl InboxEventItem {
    /// `item.provider === "linear" || item.provider === "jira"`.
    fn is_tracker(&self) -> bool {
        matches!(self.provider, InboxProvider::Linear | InboxProvider::Jira)
    }
}

/// `InboxAutomationMatch`.
#[derive(Debug, Clone, PartialEq)]
pub struct InboxAutomationMatch {
    pub automation: Automation,
    pub trigger: AutomationTrigger,
    pub item: InboxEventItem,
    pub event_key: String,
    pub occurred_at: i64,
    pub prompt: String,
}

/// `ClaimedInboxAutomationRun`.
#[derive(Debug, Clone, PartialEq)]
pub struct ClaimedInboxAutomationRun {
    pub due: DueAutomationRun,
    pub prompt: String,
    pub linked_work_item: Option<LinkedWorkItem>,
}

/// The claim `automations_claim_event` takes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationEventClaim {
    pub event_key: String,
    pub event_kind: AutomationTriggerKind,
    pub event: String,
    pub scheduled_for: i64,
    pub prompt: String,
}

/// `SUPPORTED_INBOX_TRIGGER_EVENTS`.
pub const SUPPORTED_INBOX_TRIGGER_EVENTS: [(AutomationTriggerKind, &[&str]); 5] = [
    (
        AutomationTriggerKind::Github,
        &["draft_opened", "pull_request_opened", "issue_opened"],
    ),
    (
        AutomationTriggerKind::Gitlab,
        &["merge_request_opened", "issue_opened"],
    ),
    (AutomationTriggerKind::Linear, &["issue_created"]),
    (AutomationTriggerKind::Jira, &["issue_created"]),
    (
        AutomationTriggerKind::AzureDevops,
        &["pull_request_appeared", "work_item_appeared"],
    ),
];

/// The events `SUPPORTED_INBOX_TRIGGER_EVENTS` lists for a kind.
pub fn supported_inbox_trigger_events(kind: AutomationTriggerKind) -> &'static [&'static str] {
    SUPPORTED_INBOX_TRIGGER_EVENTS
        .iter()
        .find(|(entry, _)| *entry == kind)
        .map_or(&[], |(_, events)| events)
}

/// An Inbox event a trigger can name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InboxEvent {
    pub kind: AutomationTriggerKind,
    pub event: &'static str,
}

/// `inboxAppearedEvent`.
pub fn inbox_appeared_event(item: &InboxEventItem) -> Option<InboxEvent> {
    let event = |kind, event| Some(InboxEvent { kind, event });
    match (item.provider, item.kind) {
        (InboxProvider::Github, InboxKind::Pr) => event(
            AutomationTriggerKind::Github,
            if item.draft {
                "draft_opened"
            } else {
                "pull_request_opened"
            },
        ),
        (InboxProvider::Github, InboxKind::Issue) => {
            event(AutomationTriggerKind::Github, "issue_opened")
        }
        (InboxProvider::Gitlab, InboxKind::Pr) => {
            event(AutomationTriggerKind::Gitlab, "merge_request_opened")
        }
        (InboxProvider::Gitlab, InboxKind::Issue) => {
            event(AutomationTriggerKind::Gitlab, "issue_opened")
        }
        (InboxProvider::Linear, _) => event(AutomationTriggerKind::Linear, "issue_created"),
        (InboxProvider::Jira, _) => event(AutomationTriggerKind::Jira, "issue_created"),
        (InboxProvider::AzureDevops, InboxKind::Pr) => {
            event(AutomationTriggerKind::AzureDevops, "pull_request_appeared")
        }
        (InboxProvider::AzureDevops, InboxKind::Issue) => {
            event(AutomationTriggerKind::AzureDevops, "work_item_appeared")
        }
        _ => None,
    }
}

fn provider_str(provider: InboxProvider) -> &'static str {
    match provider {
        InboxProvider::Github => "github",
        InboxProvider::Linear => "linear",
        InboxProvider::Jira => "jira",
        InboxProvider::Gitlab => "gitlab",
        InboxProvider::AzureDevops => "azuredevops",
    }
}

fn kind_str(kind: InboxKind) -> &'static str {
    match kind {
        InboxKind::Issue => "issue",
        InboxKind::Pr => "pr",
        InboxKind::Linear => "linear",
        InboxKind::Jira => "jira",
    }
}

/// `automationEventKey`: a stable, store-safe identity for the item.
pub fn automation_event_key(item: &InboxEventItem) -> String {
    let identity = if item.is_tracker() {
        let id = item
            .id
            .as_deref()
            .filter(|id| !id.is_empty())
            .or(item.identifier.as_deref().filter(|id| !id.is_empty()))
            .map_or_else(|| item.number.to_string(), str::to_string);
        format!("{}:issue:{id}", provider_str(item.provider))
    } else {
        format!(
            "{}:{}:{}:{}",
            provider_str(item.provider),
            kind_str(item.kind),
            item.repo,
            item.number
        )
    };
    let mut key = String::new();
    for c in js::trim(&identity).to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, ':' | '_' | '-' | '.' | '/')
        {
            key.push(c);
        } else {
            // The TypeScript regex replaced each UTF-16 unit.
            for _ in 0..c.len_utf16() {
                key.push('_');
            }
        }
    }
    key.truncate(400);
    key
}

/// The retry list, loaded from `Kv` on first use (the module-level
/// `retryItems` map).
#[derive(Debug, Default)]
pub struct InboxRetries {
    items: Option<Vec<(String, InboxEventItem)>>,
}

impl InboxRetries {
    /// `pendingRetryItems`.
    fn pending(&mut self, kv: &Kv) -> &mut Vec<(String, InboxEventItem)> {
        self.items.get_or_insert_with(|| {
            let mut items = Vec::new();
            // A malformed retry list should not block new Inbox events.
            let saved = kv.get_item(RETRY_STORAGE_KEY);
            if let Ok(serde_json::Value::Array(saved)) =
                serde_json::from_str(saved.as_deref().unwrap_or("[]"))
            {
                for raw in saved {
                    let Ok(item) = serde_json::from_value::<InboxEventItem>(raw) else {
                        continue;
                    };
                    let key = automation_event_key(&item);
                    if !key.is_empty() {
                        set_item(&mut items, key, item);
                    }
                }
            }
            items
        })
    }

    /// The items waiting for a retry, oldest first.
    pub fn snapshot(&mut self, kv: &Kv) -> Vec<InboxEventItem> {
        self.pending(kv)
            .iter()
            .map(|(_, item)| item.clone())
            .collect()
    }
}

/// `Map.set`: a known key keeps its position.
fn set_item(items: &mut Vec<(String, InboxEventItem)>, key: String, item: InboxEventItem) {
    match items.iter_mut().find(|(entry, _)| *entry == key) {
        Some(entry) => entry.1 = item,
        None => items.push((key, item)),
    }
}

/// `saveRetryItems`.
fn save_retry_items(kv: &Kv, items: &[(String, InboxEventItem)]) {
    if items.is_empty() {
        kv.remove_item(RETRY_STORAGE_KEY);
        return;
    }
    let values: Vec<&InboxEventItem> = items.iter().map(|(_, item)| item).collect();
    if let Ok(raw) = serde_json::to_string(&values) {
        kv.set_item(RETRY_STORAGE_KEY, &raw);
    }
}

/// `matchInboxAutomations`: each enabled automation fires at most once per
/// item, on its first matching trigger.
pub fn match_inbox_automations(
    automations: &[Automation],
    appeared: &[InboxEventItem],
) -> Vec<InboxAutomationMatch> {
    let mut matches = Vec::new();
    let mut seen = HashSet::new();
    for item in appeared {
        let Some(event) = inbox_appeared_event(item) else {
            continue;
        };
        let event_key = automation_event_key(item);
        if event_key.is_empty() {
            continue;
        }
        for automation in automations {
            if !automation.enabled {
                continue;
            }
            let Some(trigger) = automation_triggers(automation)
                .into_iter()
                .find(|candidate| {
                    trigger_matches_inbox_item(candidate, &automation.cwd, item, event)
                })
            else {
                continue;
            };
            if !seen.insert(format!("{}:{event_key}", automation.id)) {
                continue;
            }
            matches.push(InboxAutomationMatch {
                automation: automation.clone(),
                trigger,
                item: item.clone(),
                event_key: event_key.clone(),
                occurred_at: item_occurred_at(item),
                prompt: format!(
                    "{}\n\n{}",
                    js::trim(&automation.prompt),
                    js::trim(&inbox_start_draft(item))
                ),
            });
        }
    }
    matches
}

/// `claimInboxAutomationRuns`: queue the appeared items with the earlier
/// failures, claim every match, and keep only the items whose claim failed.
/// A store failure while listing automations keeps every item queued.
pub async fn claim_inbox_automation_runs(
    backend: &dyn AutomationsBackend,
    kv: &Kv,
    retries: &RefCell<InboxRetries>,
    appeared: &[InboxEventItem],
    now: i64,
) -> Result<Vec<ClaimedInboxAutomationRun>, String> {
    let candidates = {
        let mut retries = retries.borrow_mut();
        let pending = retries.pending(kv);
        for item in appeared {
            let key = automation_event_key(item);
            if !key.is_empty() {
                set_item(pending, key, item.clone());
            }
        }
        while pending.len() > MAX_RETRY_ITEMS {
            pending.remove(0);
        }
        save_retry_items(kv, pending);
        if pending.is_empty() {
            return Ok(Vec::new());
        }
        pending
            .iter()
            .map(|(_, item)| item.clone())
            .collect::<Vec<_>>()
    };

    let automations = backend.list().await?;
    let matches = match_inbox_automations(&automations, &candidates);
    let mut claimed = Vec::new();
    let mut failed = HashSet::new();
    for found in matches {
        let claim = AutomationEventClaim {
            event_key: found.event_key.clone(),
            event_kind: found.trigger.kind,
            event: found.trigger.event.clone(),
            scheduled_for: if found.occurred_at != 0 {
                found.occurred_at
            } else {
                now
            },
            prompt: found.prompt.clone(),
        };
        match backend
            .claim_event(found.automation.id.clone(), claim, now)
            .await
        {
            Ok(Some(due)) => claimed.push(ClaimedInboxAutomationRun {
                due,
                prompt: found.prompt,
                linked_work_item: linked_work_item_from_inbox_item(&found.item),
            }),
            Ok(None) => {}
            Err(_) => {
                failed.insert(found.event_key);
            }
        }
    }
    let mut retries = retries.borrow_mut();
    let pending = retries.pending(kv);
    for item in &candidates {
        let key = automation_event_key(item);
        if !failed.contains(&key) {
            pending.retain(|(entry, _)| *entry != key);
        }
    }
    save_retry_items(kv, pending);
    Ok(claimed)
}

fn trigger_matches_inbox_item(
    trigger: &AutomationTrigger,
    cwd: &str,
    item: &InboxEventItem,
    event: InboxEvent,
) -> bool {
    if trigger.kind != event.kind || trigger.event != event.event {
        return false;
    }
    if !matches_inbox_project(item, cwd) {
        return false;
    }
    if !matches_actor(&trigger.actor) {
        return false;
    }
    let repos: Vec<String> = trigger
        .repos
        .iter()
        .chain(std::iter::once(&trigger.repo))
        .map(|repo| js::trim(repo).to_lowercase())
        .filter(|repo| !repo.is_empty())
        .collect();
    if repos.is_empty() {
        return true;
    }
    repos.contains(&js::trim(&item.repo).to_lowercase())
}

fn matches_inbox_project(item: &InboxEventItem, cwd: &str) -> bool {
    // Linear and Jira issues have no git path. The automation's own project
    // is the workspace the agent should run in.
    if item.is_tracker() && js::trim(&item.project_path).is_empty() {
        return true;
    }
    same_project_path(&item.project_path, cwd)
}

/// Only "anyone" is verifiable: the Inbox does not report who opened an item.
fn matches_actor(actor: &str) -> bool {
    let value = js::trim(actor).to_lowercase();
    value.is_empty() || value == "anyone"
}

fn item_occurred_at(item: &InboxEventItem) -> i64 {
    item.created_at
        .as_deref()
        .and_then(parse_date)
        .or_else(|| parse_date(&item.updated_at))
        .unwrap_or(0)
}

/// `Date.parse` for the ISO forms the providers send: an offset or `Z` is
/// absolute, a date alone is UTC midnight, a date and time alone is local.
pub fn parse_date(value: &str) -> Option<i64> {
    let value = js::trim(value);
    if let Ok(parsed) = DateTime::parse_from_rfc3339(value) {
        return Some(parsed.timestamp_millis());
    }
    if let Ok(date) = NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        return Some(date.and_hms_opt(0, 0, 0)?.and_utc().timestamp_millis());
    }
    for format in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%dT%H:%M"] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(value, format) {
            return match Local.from_local_datetime(&naive) {
                LocalResult::Single(local) | LocalResult::Ambiguous(local, _) => {
                    Some(local.timestamp_millis())
                }
                LocalResult::None => None,
            };
        }
    }
    None
}

// Local copies of inbox package helpers (NEEDS.md).

/// `inboxStartDraft(item)` without a body.
pub fn inbox_start_draft(item: &InboxEventItem) -> String {
    if item.is_tracker() {
        let provider = if item.provider == InboxProvider::Jira {
            "Jira"
        } else {
            "Linear"
        };
        let id = item
            .identifier
            .as_deref()
            .map(js::trim)
            .filter(|id| !id.is_empty())
            .map_or_else(|| format!("{provider} #{}", item.number), str::to_string);
        let title = js::trim(&item.title);
        let title = if title.is_empty() { id.as_str() } else { title };
        let mut lines = vec![
            format!("Work on this {provider} issue:"),
            String::new(),
            format!("{id} {title}"),
        ];
        let url = js::trim(&item.url);
        if !url.is_empty() {
            lines.push(url.to_string());
        }
        return format!("{}\n", lines.join("\n"));
    }
    let kind = if item.kind == InboxKind::Pr {
        "pull request"
    } else {
        "issue"
    };
    let provider = match item.provider {
        InboxProvider::Gitlab => "GitLab",
        InboxProvider::AzureDevops => "ADO",
        _ => "GitHub",
    };
    let provider_kind = if item.provider == InboxProvider::Gitlab && item.kind == InboxKind::Pr {
        "merge request"
    } else {
        kind
    };
    let title = js::trim(&item.title);
    let title = if title.is_empty() {
        format!("{provider} {provider_kind} #{}", item.number)
    } else {
        title.to_string()
    };
    let mut lines = vec![
        format!("Work on this {provider} {provider_kind}:"),
        String::new(),
        format!("#{} {title}", item.number),
    ];
    let url = js::trim(&item.url);
    if !url.is_empty() {
        lines.push(url.to_string());
    }
    format!("{}\n", lines.join("\n"))
}

static VALID_REPO_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$").expect("valid regex"));
static AUTOMATION_EVENT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^github:(pr|issue):([^/:]+/[^/:]+):([1-9]\d*)$").expect("valid regex")
});

fn valid_number(value: i64) -> bool {
    value > 0 && value <= 9_007_199_254_740_991
}

fn github_url(repo: &str, kind: WorkItemKind, number: i64) -> String {
    format!(
        "https://github.com/{repo}/{}/{number}",
        if kind == WorkItemKind::Pr {
            "pull"
        } else {
            "issues"
        }
    )
}

fn linked(kind: WorkItemKind, repo: &str, number: i64, url: String) -> LinkedWorkItem {
    LinkedWorkItem {
        kind,
        repo: repo.to_string(),
        number,
        url,
        extra: Extra::new(),
    }
}

/// `linkedWorkItemFromInboxItem`: GitHub issues and pull requests only.
pub fn linked_work_item_from_inbox_item(item: &InboxEventItem) -> Option<LinkedWorkItem> {
    if item.provider != InboxProvider::Github
        || !valid_number(item.number)
        || !VALID_REPO_RE.is_match(&item.repo)
    {
        return None;
    }
    let kind = match item.kind {
        InboxKind::Pr => WorkItemKind::Pr,
        InboxKind::Issue => WorkItemKind::Issue,
        _ => return None,
    };
    let url = if item.url.is_empty() {
        github_url(&item.repo, kind, item.number)
    } else {
        item.url.clone()
    };
    Some(linked(kind, &item.repo, item.number, url))
}

/// `linkedWorkItemFromAutomationEvent`: the GitHub identity saved on an
/// event run.
pub fn linked_work_item_from_automation_event(run: &AutomationRun) -> Option<LinkedWorkItem> {
    if run.trigger != AutomationRunTrigger::Event
        || run.event_kind != Some(AutomationTriggerKind::Github)
    {
        return None;
    }
    let captures =
        AUTOMATION_EVENT_RE.captures(js::trim(run.event_key.as_deref().unwrap_or("")))?;
    let number: f64 = captures[3].parse().ok()?;
    if number > 9_007_199_254_740_991.0 {
        return None;
    }
    let number = number as i64;
    if !valid_number(number) {
        return None;
    }
    let kind = if captures[1].eq_ignore_ascii_case("pr") {
        WorkItemKind::Pr
    } else {
        WorkItemKind::Issue
    };
    let repo = captures[2].to_string();
    if !VALID_REPO_RE.is_match(&repo) {
        return None;
    }
    Some(linked(kind, &repo, number, github_url(&repo, kind, number)))
}
