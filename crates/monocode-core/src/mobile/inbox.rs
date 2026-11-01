//! The Inbox as a paired phone sees it: the slim, camelCase structures of
//! `docs/native/mobile-inbox-protocol.md`, and the pure steps that build them
//! from the desktop's items, read state and triage state.
//!
//! Nothing here reads a provider or a credential, and the types hold no field
//! a token could ride in. Provider errors are cut down by [`safe_error`] before
//! they cross the link.
use crate::inbox::{
    patch, timestamp_ms, Check, Checks, Details, Diff, Item, Seen, Status, Thread, Triage,
    PROVIDERS,
};
use base64::Engine;
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex},
};

pub const DEFAULT_LIMIT: usize = 30;
pub const MAX_LIMIT: usize = 100;
/// Lines per file when a diff lists every file.
pub const DIFF_PREVIEW_LINES: usize = 200;
/// Lines of one file fetched on its own.
pub const DIFF_FILE_LINES: usize = 5000;
pub const MAX_DIFF_FILES: usize = 300;
/// A cached avatar or image answer stays under this many bytes of image data.
pub const MAX_IMAGE_BYTES: usize = 3 * 1024 * 1024;
const MAX_AVATARS: usize = 4000;
const MAX_ERROR_CHARS: usize = 160;

// MARK: wire types

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Author {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar_key: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxItem {
    pub key: String,
    pub source: String,
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number: Option<String>,
    pub title: String,
    pub repo: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    pub author: Author,
    pub status: &'static str,
    pub updated_at: i64,
    pub unread: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attention: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attention_label: Option<String>,
    pub done: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snoozed_until: Option<i64>,
    pub comments: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checks: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additions: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deletions: Option<i64>,
    pub url: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderState {
    pub id: String,
    pub connected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub unread: usize,
    pub needs_me: usize,
    pub done: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListResponse {
    pub items: Vec<InboxItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    pub stale: bool,
    pub fetched_at: i64,
    pub providers: Vec<ProviderState>,
    pub counts: Counts,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadEntry {
    pub id: String,
    pub author: Author,
    pub created_at: i64,
    pub body: String,
    pub kind: &'static str,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemResponse {
    pub item: InboxItem,
    pub body: String,
    pub thread: Vec<ThreadEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_decision: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checks_summary: Option<&'static str>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    pub kind: &'static str,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_no: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_no: Option<u32>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffFile {
    pub path: String,
    pub additions: i64,
    pub deletions: i64,
    pub lines: Vec<DiffLine>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffResponse {
    pub files: Vec<DiffFile>,
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckRow {
    pub name: String,
    pub state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChecksResponse {
    pub checks: Vec<CheckRow>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaResponse {
    pub mime: String,
    pub base64: String,
}

// MARK: requests

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scope {
    #[default]
    All,
    NeedsMe,
    Done,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListParams {
    pub scope: Scope,
    pub source: Option<String>,
    pub project_id: Option<String>,
    pub q: String,
    pub cursor: Option<String>,
    pub limit: usize,
    /// The phone asked for a fresh read (pull to refresh): the host fetches from
    /// the providers and answers when that ends, instead of from the cache.
    pub refresh: bool,
}
impl ListParams {
    pub fn parse(params: &Value) -> Result<Self, String> {
        let text = |key: &str| {
            params[key]
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        };
        let scope = match params["scope"].as_str() {
            None | Some("all") => Scope::All,
            Some("needsMe") => Scope::NeedsMe,
            Some("done") => Scope::Done,
            Some(_) => return Err("Unknown inbox scope".into()),
        };
        let source = text("source");
        if let Some(source) = &source {
            if !PROVIDERS.contains(&source.as_str()) {
                return Err("Unknown inbox source".into());
            }
        }
        Ok(Self {
            scope,
            source,
            project_id: text("projectId"),
            q: text("q").unwrap_or_default(),
            cursor: text("cursor"),
            limit: params["limit"]
                .as_u64()
                .map_or(DEFAULT_LIMIT, |limit| limit as usize)
                .clamp(1, MAX_LIMIT),
            refresh: params["refresh"].as_bool().unwrap_or(false),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    Seen,
    Unseen,
    Done,
    Undone,
    Snooze(i64),
    Unsnooze,
}
impl Mark {
    pub fn parse(params: &Value) -> Result<Self, String> {
        match params["action"].as_str() {
            Some("seen") => Ok(Self::Seen),
            Some("unseen") => Ok(Self::Unseen),
            Some("done") => Ok(Self::Done),
            Some("undone") => Ok(Self::Undone),
            Some("unsnooze") => Ok(Self::Unsnooze),
            Some("snooze") => params["until"]
                .as_f64()
                .filter(|until| until.is_finite() && *until > 0.0)
                .map(|until| Self::Snooze(until as i64))
                .ok_or_else(|| "Snooze needs a time to wake at".into()),
            _ => Err("Unknown inbox action".into()),
        }
    }
    /// Applies the action to the shared read and triage state.
    pub fn apply(self, item: &Item, seen: &mut Seen, triage: &mut Triage, now: i64) {
        let key = item.key();
        match self {
            Self::Seen => seen.mark(item),
            Self::Unseen => seen.unmark(item),
            Self::Done => triage.mark_done(&key, now),
            Self::Undone => triage.undo_done(&key),
            Self::Snooze(until) => triage.snooze(&key, until),
            Self::Unsnooze => triage.unsnooze(&key),
        }
    }
}

// MARK: avatars

/// The avatar addresses the host has handed out, by opaque key. A phone asks
/// for an avatar by key, so it never names an address for the host to fetch.
#[derive(Clone, Default)]
pub struct AvatarBook(Arc<Mutex<HashMap<String, String>>>);
impl AvatarBook {
    pub fn key(&self, url: &str) -> Option<String> {
        let url = url.trim();
        if url.is_empty() {
            return None;
        }
        let key = format!("{:016x}", fnv1a(url.as_bytes()));
        let mut urls = self.0.lock().ok()?;
        if urls.len() >= MAX_AVATARS && !urls.contains_key(&key) {
            urls.clear();
        }
        urls.insert(key.clone(), url.to_owned());
        Some(key)
    }
    pub fn url(&self, key: &str) -> Option<String> {
        self.0.lock().ok()?.get(key).cloned()
    }
}
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

fn author(provider: &str, login: &str, avatar_url: &str, avatars: &AvatarBook) -> Author {
    let login = login.trim();
    let url = if avatar_url.trim().is_empty() && provider == "github" && !login.is_empty() {
        format!("https://github.com/{login}.png?size=64")
    } else {
        avatar_url.trim().to_owned()
    };
    Author {
        name: login.to_owned(),
        avatar_key: avatars.key(&url),
    }
}

// MARK: attention

/// Why an item needs the user: the phone's attention id and its label.
pub fn attention(item: &Item) -> Option<(&'static str, String)> {
    let reason = item.attention_reason.trim().to_lowercase();
    let (id, label) = match reason.as_str() {
        "" => return None,
        "review_requested" | "approval_required" => ("reviewRequested", "Review requested"),
        "mentioned" | "directly_addressed" => ("mentioned", "Mentioned you"),
        "assigned" => ("assigned", "Assigned to you"),
        "build_failed" => ("ciFailed", "CI failed"),
        _ => {
            let words = reason.replace('_', " ");
            let mut chars = words.chars();
            let label = chars
                .next()
                .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default();
            return Some(("other", label));
        }
    };
    Some((id, label.into()))
}

fn open_status(item: &Item) -> bool {
    matches!(item.status(), "Open" | "Draft")
}

/// Whether the item is waiting on the user: it has an attention reason and is
/// still open.
pub fn needs_me(item: &Item) -> bool {
    open_status(item) && attention(item).is_some()
}

/// The four reasons worth a push: an item that newly appears for one of them.
pub fn pushes(item: &Item) -> bool {
    open_status(item) && matches!(attention(item), Some((id, _)) if id != "other")
}

/// The push for an item that newly needs the user: its title, the reason, and
/// where the project name goes.
pub fn notice(item: &Item) -> (String, String, String) {
    let who = item.author.login.trim();
    let by = |verb: &str| {
        if who.is_empty() {
            verb.to_owned()
        } else {
            format!("{verb} by {who}")
        }
    };
    let body = match attention(item) {
        Some(("reviewRequested", _)) => by("Review requested"),
        Some(("mentioned", _)) => by("Mentioned"),
        Some(("assigned", _)) => "Assigned to you".into(),
        Some(("ciFailed", _)) => "CI failed".into(),
        Some((_, label)) => label,
        None => String::new(),
    };
    (item.title.clone(), body, repo_label(item))
}

fn repo_label(item: &Item) -> String {
    [&item.repo, &item.team_name, &item.project_name]
        .into_iter()
        .find(|label| !label.trim().is_empty())
        .cloned()
        .unwrap_or_default()
}

// MARK: projects

/// A project the phone knows: its catalog id and folder.
#[derive(Clone, Debug)]
pub struct ProjectRef {
    pub id: String,
    pub path: String,
}

/// The phone project an item belongs to. An item is mapped by the checkout it
/// was fetched through; one with none (a Linear or Jira issue) has no project.
pub fn project_for(item: &Item, projects: &[ProjectRef]) -> Option<String> {
    if item.project_path.is_empty() {
        return None;
    }
    let key = crate::files::path_key(&item.project_path);
    projects
        .iter()
        .find(|project| crate::files::path_key(&project.path) == key)
        .map(|project| project.id.clone())
}

// MARK: listing

/// What a list is built against: the read and triage state, the projects the
/// phone knows and the time.
pub struct View<'a> {
    pub seen: &'a Seen,
    pub triage: &'a Triage,
    pub projects: &'a [ProjectRef],
    pub now: i64,
}
impl View<'_> {
    fn hidden(&self, item: &Item) -> bool {
        self.triage.snoozed_until(&item.key(), self.now).is_some()
    }
    fn done(&self, item: &Item) -> bool {
        self.triage.is_done(&item.key())
    }
}

fn matches_query(item: &Item, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let text = format!(
        "{} {} {} {} {} {}",
        item.title,
        item.reference(),
        item.repo,
        item.team_name,
        item.author.login,
        item.labels
            .iter()
            .map(|label| label.name.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    )
    .to_lowercase();
    query
        .split_whitespace()
        .all(|word| text.contains(&word.to_lowercase()))
}

/// Items for the source and project the phone asked about, whatever their scope.
fn in_slice<'a>(items: &'a [Item], params: &ListParams, view: &View) -> Vec<&'a Item> {
    items
        .iter()
        .filter(|item| params.source.as_ref().is_none_or(|s| *s == item.provider))
        .filter(|item| {
            params.project_id.as_ref().is_none_or(|id| {
                project_for(item, view.projects).is_some_and(|project| project == *id)
            })
        })
        .collect()
}

/// The items of the scope, newest first. A snoozed item is out of every scope
/// until its time passes; a finished one is only in `Done`.
pub fn visible<'a>(items: &'a [Item], params: &ListParams, view: &View) -> Vec<&'a Item> {
    let mut visible: Vec<&Item> = in_slice(items, params, view)
        .into_iter()
        .filter(|item| !view.hidden(item))
        .filter(|item| match params.scope {
            Scope::All => !view.done(item),
            Scope::NeedsMe => !view.done(item) && needs_me(item),
            Scope::Done => view.done(item),
        })
        .filter(|item| matches_query(item, &params.q))
        .collect();
    sort(&mut visible);
    visible
}

pub fn counts(items: &[Item], params: &ListParams, view: &View) -> Counts {
    let mut counts = Counts::default();
    for item in in_slice(items, params, view) {
        if view.hidden(item) {
            continue;
        }
        if view.done(item) {
            counts.done += 1;
            continue;
        }
        counts.unread += usize::from(view.seen.unseen(item));
        counts.needs_me += usize::from(needs_me(item));
    }
    counts
}

fn sort(items: &mut [&Item]) {
    items.sort_by_cached_key(|item| (std::cmp::Reverse(item.updated_ms()), item.key()));
}

fn encode_cursor(item: &Item) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(format!(
        "v1|{}|{}",
        item.updated_ms(),
        item.key()
    ))
}
fn decode_cursor(cursor: &str) -> Result<(i64, String), String> {
    let invalid = || "Invalid inbox cursor".to_string();
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|_| invalid())?;
    let text = String::from_utf8(bytes).map_err(|_| invalid())?;
    let mut parts = text.splitn(3, '|');
    match (parts.next(), parts.next(), parts.next()) {
        (Some("v1"), Some(at), Some(key)) => Ok((at.parse().map_err(|_| invalid())?, key.into())),
        _ => Err(invalid()),
    }
}

/// One page of already sorted items after `cursor`, and the cursor for the
/// next. The cursor names the last item sent, so items arriving or leaving
/// between pages never repeat or skip one.
pub fn page<'a>(
    items: Vec<&'a Item>,
    cursor: Option<&str>,
    limit: usize,
) -> Result<(Vec<&'a Item>, Option<String>), String> {
    let after = cursor.map(decode_cursor).transpose()?;
    let mut rest = items.into_iter().filter(|item| match &after {
        None => true,
        Some((at, key)) => {
            let own = item.updated_ms();
            own < *at || (own == *at && item.key() > *key)
        }
    });
    let page: Vec<&Item> = rest.by_ref().take(limit).collect();
    let next = (rest.next().is_some())
        .then(|| page.last().map(|item| encode_cursor(item)))
        .flatten();
    Ok((page, next))
}

/// A snapshot older than its polling interval, or one with a failed provider,
/// is stale; so is one never fetched.
pub fn is_stale(fetched_at: Option<i64>, now: i64, interval_ms: i64, failed: bool) -> bool {
    failed || fetched_at.is_none_or(|at| now - at >= interval_ms)
}

pub fn providers(
    connections: &BTreeMap<String, Status>,
    errors: &BTreeMap<String, String>,
) -> Vec<ProviderState> {
    PROVIDERS
        .iter()
        .map(|id| ProviderState {
            id: (*id).into(),
            connected: connections.get(*id).is_some_and(|s| s.connected)
                && !errors.contains_key(*id),
            error: errors.get(*id).map(|error| safe_error(error)),
        })
        .collect()
}

/// A provider's error cut to one short line with anything shaped like a
/// credential removed, since provider and `gh` output can echo a header or URL.
pub fn safe_error(error: &str) -> String {
    let secret = |word: &str| {
        let lower = word.to_lowercase();
        let long = word.len() >= 20
            && word
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "_-=+/.:".contains(c))
            && word.chars().any(|c| c.is_ascii_digit());
        long || lower.contains("token")
            || lower.contains("password")
            || lower.contains("-config.json")
            || lower.starts_with("ghp_")
            || lower.starts_with("gho_")
            || lower.starts_with("github_pat_")
            || lower.starts_with("glpat-")
            || lower.starts_with("lin_api_")
            || (word.contains("://") && word.contains('@'))
    };
    let line = error.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let mut words: Vec<&str> = vec![];
    let mut after_scheme = false;
    for word in line.split_whitespace() {
        let lower = word.to_lowercase();
        // The word after an auth scheme is the credential itself.
        let hidden = after_scheme || secret(word) || matches!(lower.as_str(), "bearer" | "basic");
        after_scheme = matches!(lower.as_str(), "bearer" | "basic");
        if hidden && words.last() == Some(&"…") {
            continue;
        }
        words.push(if hidden { "…" } else { word });
    }
    let mut out = words.join(" ");
    if out.chars().count() > MAX_ERROR_CHARS {
        out = out.chars().take(MAX_ERROR_CHARS).collect::<String>() + "…";
    }
    out
}

// MARK: slimming

pub fn slim_item(item: &Item, view: &View, avatars: &AvatarBook) -> InboxItem {
    let key = item.key();
    let (attention, attention_label) = match attention(item) {
        Some((id, label)) => (Some(id), Some(label)),
        None => (None, None),
    };
    InboxItem {
        source: item.provider.clone(),
        kind: if item.kind == "pr" { "pr" } else { "issue" },
        number: Some(item.reference()).filter(|n| n != "#0"),
        title: item.title.clone(),
        repo: repo_label(item),
        project_id: project_for(item, view.projects),
        author: author(
            &item.provider,
            &item.author.login,
            &item.author.avatar_url,
            avatars,
        ),
        status: match item.status() {
            "Merged" => "merged",
            "Closed" => "closed",
            "Draft" => "draft",
            _ => "open",
        },
        updated_at: item.updated_ms(),
        unread: view.seen.unseen(item),
        attention,
        attention_label,
        done: view.triage.is_done(&key),
        snoozed_until: view.triage.snoozed_until(&key, view.now),
        comments: 0,
        checks: None,
        additions: None,
        deletions: None,
        url: item.url.clone(),
        key,
    }
}

pub fn list_response(
    items: &[Item],
    params: &ListParams,
    view: &View,
    avatars: &AvatarBook,
    state: &SnapshotState,
) -> Result<ListResponse, String> {
    let (page, next_cursor) = page(
        visible(items, params, view),
        params.cursor.as_deref(),
        params.limit,
    )?;
    Ok(ListResponse {
        items: page
            .into_iter()
            .map(|item| slim_item(item, view, avatars))
            .collect(),
        next_cursor,
        stale: is_stale(
            state.fetched_at,
            view.now,
            state.interval_ms,
            !state.errors.is_empty(),
        ),
        fetched_at: state.fetched_at.unwrap_or(0),
        providers: providers(state.connections, state.errors),
        counts: counts(items, params, view),
    })
}

/// What a list needs to know about the snapshot beyond its items.
pub struct SnapshotState<'a> {
    pub fetched_at: Option<i64>,
    pub interval_ms: i64,
    pub connections: &'a BTreeMap<String, Status>,
    pub errors: &'a BTreeMap<String, String>,
}

pub fn review_decision(details: &Details) -> Option<&'static str> {
    match details.review_decision.trim().to_ascii_uppercase().as_str() {
        "APPROVED" => Some("approved"),
        "CHANGES_REQUESTED" => Some("changesRequested"),
        "REVIEW_REQUIRED" | "PENDING" => Some("pending"),
        _ => None,
    }
}

fn check_state(state: &str) -> &'static str {
    match state {
        "pass" => "passing",
        "fail" => "failing",
        "pending" => "pending",
        _ => "skipped",
    }
}

pub fn checks_summary(checks: &Checks) -> Option<&'static str> {
    let states: Vec<_> = checks
        .checks
        .iter()
        .map(|check| check_state(&check.state))
        .filter(|state| *state != "skipped")
        .collect();
    if states.contains(&"failing") {
        Some("failing")
    } else if states.contains(&"pending") {
        Some("pending")
    } else if states.is_empty() {
        None
    } else {
        Some("passing")
    }
}

fn duration_ms(check: &Check) -> Option<i64> {
    let (start, end) = (
        timestamp_ms(check.started_at.as_deref()?),
        timestamp_ms(check.completed_at.as_deref()?),
    );
    (start > 0 && end >= start).then_some(end - start)
}

pub fn checks_response(checks: &Checks) -> ChecksResponse {
    ChecksResponse {
        checks: checks
            .checks
            .iter()
            .map(|check| CheckRow {
                name: if check.workflow.is_empty() {
                    check.name.clone()
                } else {
                    format!("{} / {}", check.workflow, check.name)
                },
                state: check_state(&check.state),
                url: check.url.clone().filter(|url| !url.is_empty()),
                duration_ms: duration_ms(check),
            })
            .collect(),
    }
}

/// The discussion in time order: comments and replies, reviews and commits.
pub fn thread_entries(item: &Item, thread: &Thread, avatars: &AvatarBook) -> Vec<ThreadEntry> {
    let mut entries = vec![];
    fn push(
        item: &Item,
        comment: &crate::inbox::Comment,
        avatars: &AvatarBook,
        out: &mut Vec<ThreadEntry>,
    ) {
        out.push(ThreadEntry {
            id: comment.id.clone(),
            author: author(
                &item.provider,
                &comment.author,
                &comment.author_avatar_url,
                avatars,
            ),
            created_at: timestamp_ms(&comment.created_at),
            body: comment.body.clone(),
            kind: if comment.kind == "review" {
                "review"
            } else {
                "comment"
            },
        });
        for reply in &comment.replies {
            push(item, reply, avatars, out);
        }
    }
    for comment in &thread.comments {
        push(item, comment, avatars, &mut entries);
    }
    for commit in &thread.commits {
        entries.push(ThreadEntry {
            id: commit.oid.clone(),
            author: author(&item.provider, &commit.author, "", avatars),
            created_at: timestamp_ms(&commit.committed_date),
            body: commit.message_headline.clone(),
            kind: "commit",
        });
    }
    entries.sort_by_key(|entry| entry.created_at);
    entries
}

pub fn item_response(
    item: &Item,
    view: &View,
    avatars: &AvatarBook,
    details: Option<&Details>,
    thread: Option<&Thread>,
    checks: Option<&Checks>,
) -> ItemResponse {
    let mut slim = slim_item(item, view, avatars);
    let thread = thread
        .map(|thread| thread_entries(item, thread, avatars))
        .unwrap_or_default();
    slim.comments = thread.iter().filter(|e| e.kind != "commit").count();
    slim.checks = checks.and_then(checks_summary);
    ItemResponse {
        body: details.map(|d| d.body.clone()).unwrap_or_default(),
        review_decision: details.and_then(review_decision),
        checks_summary: slim.checks,
        item: slim,
        thread,
    }
}

// MARK: diff

fn line_kind(kind: crate::diff::LineKind) -> &'static str {
    match kind {
        crate::diff::LineKind::Add => "add",
        crate::diff::LineKind::Del => "del",
        crate::diff::LineKind::Context => "context",
        crate::diff::LineKind::Hunk => "hunk",
    }
}

/// Every file with its first lines, or one named file with up to a long file's
/// worth. `truncated` says whether anything was left out.
pub fn diff_response(diff: &Diff, only: Option<&str>) -> Result<DiffResponse, String> {
    let sections = patch::sections(&diff.patch);
    let cap = if only.is_some() {
        DIFF_FILE_LINES
    } else {
        DIFF_PREVIEW_LINES
    };
    let mut truncated = diff.truncated || diff.files.len() > MAX_DIFF_FILES;
    let mut files = vec![];
    for (ix, file) in diff.files.iter().enumerate() {
        if only.is_some_and(|path| path != file.path) {
            continue;
        }
        if files.len() >= MAX_DIFF_FILES {
            break;
        }
        let section = patch::section_for(&sections, &file.path, ix);
        let all: Vec<_> = if patch::is_binary(section) {
            vec![]
        } else {
            patch::blocks(section)
                .iter()
                .flat_map(|block| block.lines().iter())
                .map(|line| DiffLine {
                    kind: line_kind(line.kind),
                    text: line.text.clone(),
                    old_no: line.old_number,
                    new_no: line.new_number,
                })
                .collect()
        };
        if all.len() > cap {
            truncated = true;
        }
        files.push(DiffFile {
            path: file.path.clone(),
            additions: file.additions.max(0),
            deletions: file.deletions.max(0),
            lines: all.into_iter().take(cap).collect(),
        });
    }
    if let Some(path) = only {
        if files.is_empty() {
            return Err(format!("{path} is not in this diff"));
        }
    }
    Ok(DiffResponse { files, truncated })
}

// MARK: media

/// Image addresses in a markdown body, in the order they appear: `![alt](url)`
/// and `<img src="url">`. A phone asks for the nth by index.
pub fn image_urls(markdown: &str) -> Vec<String> {
    let mut found: Vec<(usize, String)> = vec![];
    let mut at = 0;
    while let Some(start) = markdown[at..].find("![") {
        let open = at + start;
        at = open + 2;
        let Some(close) = markdown[at..].find("](") else {
            break;
        };
        let url_start = at + close + 2;
        let Some(end) = markdown[url_start..].find(')') else {
            break;
        };
        let url = markdown[url_start..url_start + end]
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim_matches(['<', '>']);
        if !url.is_empty() {
            found.push((open, url.to_owned()));
        }
        at = url_start + end;
    }
    let lower = markdown.to_ascii_lowercase();
    let mut at = 0;
    while let Some(start) = lower[at..].find("<img") {
        let open = at + start;
        let end = lower[open..].find('>').map_or(lower.len(), |e| open + e);
        let tag = &markdown[open..end];
        if let Some(src) = tag.to_ascii_lowercase().find("src=") {
            let rest = &tag[src + 4..];
            if let Some(quote) = rest.chars().next().filter(|c| *c == '"' || *c == '\'') {
                if let Some(stop) = rest[1..].find(quote) {
                    found.push((open, rest[1..1 + stop].to_owned()));
                }
            }
        }
        at = end.max(open + 4);
    }
    found.sort_by_key(|(at, _)| *at);
    found.into_iter().map(|(_, url)| url).collect()
}

pub fn media_response(mime: &str, bytes: &[u8]) -> Result<MediaResponse, String> {
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err("Image is too large".into());
    }
    Ok(MediaResponse {
        mime: mime.into(),
        base64: base64::engine::general_purpose::STANDARD.encode(bytes),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inbox::{Comment, Commit, DiffFile as CoreDiffFile, Person};
    use serde_json::json;

    const NOW: i64 = 1_800_000_000_000;
    const HOUR: i64 = 3_600_000;

    fn ms(at: i64) -> String {
        chrono::DateTime::from_timestamp_millis(at)
            .unwrap()
            .to_rfc3339()
    }
    fn item(number: i64, age_hours: i64, reason: &str) -> Item {
        Item {
            provider: "github".into(),
            kind: "pr".into(),
            repo: "acme/app".into(),
            number,
            id: number.to_string(),
            title: format!("Change {number}"),
            url: format!("https://github.com/acme/app/pull/{number}"),
            state: "open".into(),
            updated_at: ms(NOW - age_hours * HOUR),
            attention_reason: reason.into(),
            project_path: "/work/app".into(),
            author: Person {
                login: "sam".into(),
                avatar_url: "https://avatars.example/sam.png".into(),
            },
            ..Item::default()
        }
    }
    fn projects() -> Vec<ProjectRef> {
        vec![ProjectRef {
            id: "%2Fwork%2Fapp".into(),
            path: "/work/app".into(),
        }]
    }
    fn view<'a>(seen: &'a Seen, triage: &'a Triage, projects: &'a [ProjectRef]) -> View<'a> {
        View {
            seen,
            triage,
            projects,
            now: NOW,
        }
    }
    fn params(value: Value) -> ListParams {
        ListParams::parse(&value).unwrap()
    }
    fn numbers(items: &[&Item]) -> Vec<i64> {
        items.iter().map(|i| i.number).collect()
    }

    #[test]
    fn scopes_filter_all_needs_me_and_done() {
        let items = vec![
            item(1, 1, "review_requested"),
            item(2, 2, ""),
            item(3, 3, "mentioned"),
            item(4, 4, "assigned"),
        ];
        let mut closed = item(5, 5, "assigned");
        closed.state = "closed".into();
        let items = [items, vec![closed]].concat();
        let mut triage = Triage::default();
        triage.mark_done(&items[3].key(), NOW);
        let (seen, projects) = (Seen::default(), projects());
        let view = view(&seen, &triage, &projects);
        let at = |scope: &str| numbers(&visible(&items, &params(json!({"scope": scope})), &view));
        assert_eq!(at("all"), [1, 2, 3, 5]);
        assert_eq!(at("needsMe"), [1, 3]);
        assert_eq!(at("done"), [4]);
        assert_eq!(
            numbers(&visible(&items, &params(json!({})), &view)),
            [1, 2, 3, 5]
        );
        assert_eq!(
            counts(&items, &params(json!({})), &view),
            Counts {
                unread: 0,
                needs_me: 2,
                done: 1
            }
        );
    }

    #[test]
    fn a_list_asks_for_a_fresh_read_only_when_told_to() {
        assert!(!params(json!({})).refresh);
        assert!(!params(json!({"refresh": "yes"})).refresh);
        assert!(params(json!({"refresh": true})).refresh);
    }

    #[test]
    fn source_project_and_query_narrow_the_list() {
        let mut linear = item(7, 1, "assigned");
        linear.provider = "linear".into();
        linear.project_path = String::new();
        linear.identifier = "ENG-7".into();
        linear.title = "Crash on launch".into();
        let mut other = item(8, 2, "");
        other.project_path = "/work/site".into();
        let items = vec![item(1, 3, ""), linear, other];
        let (seen, triage, projects) = (Seen::default(), Triage::default(), projects());
        let view = view(&seen, &triage, &projects);
        let at = |p: Value| numbers(&visible(&items, &params(p), &view));
        assert_eq!(at(json!({"source": "linear"})), [7]);
        assert_eq!(at(json!({"projectId": "%2Fwork%2Fapp"})), [1]);
        assert_eq!(at(json!({"projectId": "unknown"})), Vec::<i64>::new());
        assert_eq!(at(json!({})), [7, 8, 1]);
        assert_eq!(at(json!({"q": "crash launch"})), [7]);
        assert_eq!(at(json!({"q": "eng-7"})), [7]);
        assert!(ListParams::parse(&json!({"scope": "x"})).is_err());
        assert!(ListParams::parse(&json!({"source": "svn"})).is_err());
        assert_eq!(params(json!({"limit": 9999})).limit, MAX_LIMIT);
        assert_eq!(params(json!({})).limit, DEFAULT_LIMIT);
    }

    #[test]
    fn items_map_to_the_phone_project_of_their_checkout() {
        let projects = projects();
        assert_eq!(
            project_for(&item(1, 1, ""), &projects).as_deref(),
            Some("%2Fwork%2Fapp")
        );
        let mut unmapped = item(2, 1, "");
        unmapped.project_path = "/elsewhere".into();
        assert_eq!(project_for(&unmapped, &projects), None);
        unmapped.project_path = String::new();
        assert_eq!(project_for(&unmapped, &projects), None);
    }

    #[test]
    fn pages_walk_the_list_with_opaque_cursors_without_repeats() {
        let items: Vec<Item> = (1..=7).map(|n| item(n, n, "")).collect();
        let (seen, triage, projects) = (Seen::default(), Triage::default(), projects());
        let view = view(&seen, &triage, &projects);
        let mut cursor: Option<String> = None;
        let mut seen_numbers = vec![];
        let mut pages = 0;
        loop {
            let (page, next) = page(
                visible(&items, &params(json!({})), &view),
                cursor.as_deref(),
                3,
            )
            .unwrap();
            seen_numbers.extend(numbers(&page));
            pages += 1;
            match next {
                Some(next) => {
                    assert!(!next.contains('|') && !next.contains("github"), "opaque");
                    cursor = Some(next);
                }
                None => break,
            }
        }
        assert_eq!((pages, seen_numbers), (3, vec![1, 2, 3, 4, 5, 6, 7]));
        assert!(page(vec![], Some("garbage!"), 3).is_err());
    }

    #[test]
    fn a_cursor_survives_the_list_changing_between_pages() {
        let mut items: Vec<Item> = (1..=6).map(|n| item(n, n, "")).collect();
        let (seen, projects) = (Seen::default(), projects());
        let mut triage = Triage::default();
        let (first, next) = {
            let view = view(&seen, &triage, &projects);
            let (first, next) = page(visible(&items, &params(json!({})), &view), None, 3).unwrap();
            (numbers(&first), next.unwrap())
        };
        assert_eq!(first, [1, 2, 3]);
        // The last item sent goes away and a newer one arrives.
        triage.mark_done(&items[2].key(), NOW);
        items.push(item(9, 0, ""));
        let view = view(&seen, &triage, &projects);
        let (second, _) = page(visible(&items, &params(json!({})), &view), Some(&next), 3).unwrap();
        assert_eq!(numbers(&second), [4, 5, 6]);
    }

    #[test]
    fn stale_means_old_failed_or_never_fetched() {
        let interval = 120_000;
        assert!(is_stale(None, NOW, interval, false));
        assert!(!is_stale(Some(NOW - 1000), NOW, interval, false));
        assert!(is_stale(Some(NOW - interval), NOW, interval, false));
        assert!(is_stale(Some(NOW - 1000), NOW, interval, true));
    }

    #[test]
    fn list_response_reports_stale_providers_and_counts() {
        let items = vec![item(1, 1, "review_requested"), item(2, 2, "")];
        let (mut seen, triage, projects) = (Seen::default(), Triage::default(), projects());
        seen.seeded = true;
        seen.items.insert(items[1].key(), items[1].updated_ms());
        let view = view(&seen, &triage, &projects);
        let mut connections = BTreeMap::new();
        connections.insert(
            "github".to_string(),
            Status {
                connected: true,
                ..Status::default()
            },
        );
        let mut errors = BTreeMap::new();
        errors.insert("linear".to_string(), "offline".to_string());
        let state = SnapshotState {
            fetched_at: Some(NOW - 1000),
            interval_ms: 120_000,
            connections: &connections,
            errors: &errors,
        };
        let response = list_response(
            &items,
            &params(json!({})),
            &view,
            &AvatarBook::default(),
            &state,
        )
        .unwrap();
        assert!(response.stale, "a failed provider makes it stale");
        assert_eq!(response.fetched_at, NOW - 1000);
        assert_eq!(
            response.counts,
            Counts {
                unread: 1,
                needs_me: 1,
                done: 0
            }
        );
        assert!(response.items[0].unread && !response.items[1].unread);
        let github = &response.providers[0];
        assert_eq!((github.id.as_str(), github.connected), ("github", true));
        let linear = &response.providers[1];
        assert_eq!(linear.error.as_deref(), Some("offline"));
        assert!(!linear.connected);
        let fresh = SnapshotState {
            errors: &BTreeMap::new(),
            ..state
        };
        let response = list_response(
            &items,
            &params(json!({})),
            &view,
            &AvatarBook::default(),
            &fresh,
        )
        .unwrap();
        assert!(!response.stale);
    }

    #[test]
    fn a_snoozed_item_is_hidden_until_its_time_then_reappears() {
        let items = vec![item(1, 1, "mentioned"), item(2, 2, "")];
        let (seen, projects) = (Seen::default(), projects());
        let mut triage = Triage::default();
        triage.snooze(&items[0].key(), NOW + HOUR);
        let at = |now: i64, triage: &Triage| {
            let view = View {
                seen: &seen,
                triage,
                projects: &projects,
                now,
            };
            (
                numbers(&visible(&items, &params(json!({})), &view)),
                numbers(&visible(
                    &items,
                    &params(json!({"scope": "needsMe"})),
                    &view,
                )),
                counts(&items, &params(json!({})), &view).needs_me,
            )
        };
        assert_eq!(at(NOW, &triage), (vec![2], vec![], 0));
        assert_eq!(at(NOW + HOUR - 1, &triage).0, [2]);
        assert_eq!(at(NOW + HOUR, &triage), (vec![1, 2], vec![1], 1));
        triage.prune(NOW + HOUR);
        assert!(triage.snoozed.is_empty());
        let view = View {
            seen: &seen,
            triage: &triage,
            projects: &projects,
            now: NOW,
        };
        assert_eq!(numbers(&visible(&items, &params(json!({})), &view)), [1, 2]);
    }

    #[test]
    fn marks_change_the_shared_read_and_triage_state() {
        let one = item(1, 1, "");
        let (mut seen, mut triage) = (
            Seen {
                seeded: true,
                ..Seen::default()
            },
            Triage::default(),
        );
        assert!(seen.unseen(&one));
        Mark::Seen.apply(&one, &mut seen, &mut triage, NOW);
        assert!(!seen.unseen(&one));
        Mark::Unseen.apply(&one, &mut seen, &mut triage, NOW);
        assert!(seen.unseen(&one));
        Mark::Snooze(NOW + HOUR).apply(&one, &mut seen, &mut triage, NOW);
        assert_eq!(triage.snoozed_until(&one.key(), NOW), Some(NOW + HOUR));
        Mark::Done.apply(&one, &mut seen, &mut triage, NOW);
        assert!(triage.is_done(&one.key()) && triage.snoozed.is_empty());
        Mark::Undone.apply(&one, &mut seen, &mut triage, NOW);
        assert!(!triage.is_done(&one.key()));
        assert_eq!(
            Mark::parse(&json!({"action": "snooze", "until": 5000})),
            Ok(Mark::Snooze(5000))
        );
        assert!(Mark::parse(&json!({"action": "snooze"})).is_err());
        assert!(Mark::parse(&json!({"action": "merge"})).is_err());
    }

    #[test]
    fn triage_survives_a_save_and_load_of_the_view_state() {
        let one = item(1, 1, "");
        let mut state = crate::inbox::ViewState::default();
        state.triage.mark_done(&one.key(), NOW);
        state.triage.snooze("github:acme/app:pr:2", NOW + HOUR);
        let saved = serde_json::to_string(&state).unwrap();
        let loaded: crate::inbox::ViewState = serde_json::from_str(&saved).unwrap();
        assert!(loaded.triage.is_done(&one.key()));
        assert_eq!(
            loaded.triage.snoozed_until("github:acme/app:pr:2", NOW),
            Some(NOW + HOUR)
        );
        // State saved before triage existed still loads.
        let old: crate::inbox::ViewState =
            serde_json::from_str(r#"{"source":"github","seen":{"seeded":true,"items":{}}}"#)
                .unwrap();
        assert_eq!(old.triage, Triage::default());
    }

    #[test]
    fn triage_persists_through_the_settings_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::store::Store::open(&dir.path().join("inbox.db")).unwrap();
        let one = item(1, 1, "");
        let mut state = crate::inbox::ViewState::load(&store);
        state.triage.mark_done(&one.key(), NOW);
        state.save(&store).unwrap();
        assert!(crate::inbox::ViewState::load(&store)
            .triage
            .is_done(&one.key()));
    }

    #[test]
    fn attention_reasons_map_to_the_phone_ids() {
        let reason = |r: &str| attention(&item(1, 1, r)).map(|(id, _)| id);
        assert_eq!(reason("review_requested"), Some("reviewRequested"));
        assert_eq!(reason("approval_required"), Some("reviewRequested"));
        assert_eq!(reason("directly_addressed"), Some("mentioned"));
        assert_eq!(reason("assigned"), Some("assigned"));
        assert_eq!(reason("build_failed"), Some("ciFailed"));
        assert_eq!(reason("unmergeable"), Some("other"));
        assert_eq!(reason(""), None);
        assert_eq!(
            attention(&item(1, 1, "member_access_requested")).unwrap().1,
            "Member access requested"
        );
        assert!(pushes(&item(1, 1, "mentioned")));
        assert!(!pushes(&item(1, 1, "marked")));
        assert!(!pushes(&item(1, 1, "")));
        let mut merged = item(1, 1, "mentioned");
        merged.state = "merged".into();
        assert!(!pushes(&merged));
    }

    #[test]
    fn a_push_carries_the_title_and_the_reason() {
        let (title, body, repo) = notice(&item(12, 1, "review_requested"));
        assert_eq!(
            (title.as_str(), body.as_str(), repo.as_str()),
            ("Change 12", "Review requested by sam", "acme/app")
        );
        assert_eq!(notice(&item(1, 1, "build_failed")).1, "CI failed");
        assert_eq!(notice(&item(1, 1, "assigned")).1, "Assigned to you");
        let mut anonymous = item(1, 1, "mentioned");
        anonymous.author = Person::default();
        assert_eq!(notice(&anonymous).1, "Mentioned");
    }

    #[test]
    fn only_new_items_that_need_the_user_are_pushed() {
        use crate::inbox::{NotificationTracker, Snapshot};
        let snapshot = |items: Vec<Item>| Snapshot {
            items,
            ..Snapshot::default()
        };
        let mut tracker = NotificationTracker::default();
        // The backlog is a baseline: nothing in it is announced, even if it needs me.
        assert!(tracker
            .observe(&snapshot(vec![item(1, 5, "review_requested")]), "s".into())
            .is_empty());
        let mut merged = item(5, 1, "mentioned");
        merged.state = "merged".into();
        let appeared = tracker.observe(
            &snapshot(vec![
                item(1, 5, "review_requested"),
                item(2, 1, "review_requested"),
                item(3, 1, ""),
                item(4, 1, "marked"),
                merged,
                item(6, 1, "build_failed"),
            ]),
            "s".into(),
        );
        let pushed: Vec<_> = appeared
            .iter()
            .filter(|i| pushes(i))
            .map(|i| i.number)
            .collect();
        assert_eq!(pushed, [2, 6]);
        // An update to an item already seen is not new.
        let again = tracker.observe(&snapshot(vec![item(2, 0, "review_requested")]), "s".into());
        assert!(again.iter().all(|i| !pushes(i)));
    }

    #[test]
    fn slim_items_follow_the_contract_shape() {
        let (seen, triage, projects) = (Seen::default(), Triage::default(), projects());
        let view = view(&seen, &triage, &projects);
        let avatars = AvatarBook::default();
        let mut draft = item(12, 1, "review_requested");
        draft.draft = true;
        let slim = serde_json::to_value(slim_item(&draft, &view, &avatars)).unwrap();
        assert_eq!(slim["key"], "github:acme/app:pr:12");
        assert_eq!(slim["source"], "github");
        assert_eq!(slim["kind"], "pr");
        assert_eq!(slim["number"], "#12");
        assert_eq!(slim["status"], "draft");
        assert_eq!(slim["attention"], "reviewRequested");
        assert_eq!(slim["attentionLabel"], "Review requested");
        assert_eq!(slim["projectId"], "%2Fwork%2Fapp");
        assert_eq!(slim["updatedAt"], draft.updated_ms());
        assert_eq!(slim["done"], false);
        assert_eq!(slim["author"]["name"], "sam");
        let key = slim["author"]["avatarKey"].as_str().unwrap();
        assert_eq!(
            avatars.url(key).as_deref(),
            Some("https://avatars.example/sam.png")
        );
        assert!(slim.get("snoozedUntil").is_none() && slim.get("checks").is_none());
        // A GitHub login with no address falls back to the public avatar.
        let mut bare = item(1, 1, "");
        bare.author.avatar_url = String::new();
        let slim = slim_item(&bare, &view, &avatars);
        assert_eq!(
            avatars.url(&slim.author.avatar_key.unwrap()).as_deref(),
            Some("https://github.com/sam.png?size=64")
        );
    }

    fn sample_diff() -> Diff {
        let mut body = String::new();
        let mut patch = String::from(
            "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1,1 +1,300 @@\n x\n",
        );
        for n in 0..299 {
            body.push_str(&format!("+line {n}\n"));
        }
        patch.push_str(&body);
        patch.push_str(
            "diff --git a/b.rs b/b.rs\n--- a/b.rs\n+++ b/b.rs\n@@ -1 +1 @@\n-old\n+new\n",
        );
        Diff {
            additions: 300,
            deletions: 1,
            files: vec![
                CoreDiffFile {
                    path: "a.rs".into(),
                    additions: 299,
                    deletions: 0,
                },
                CoreDiffFile {
                    path: "b.rs".into(),
                    additions: 1,
                    deletions: 1,
                },
            ],
            patch,
            truncated: false,
        }
    }

    #[test]
    fn a_diff_lists_files_with_a_preview_and_one_file_in_full() {
        let diff = sample_diff();
        let list = diff_response(&diff, None).unwrap();
        assert!(list.truncated);
        assert_eq!(list.files[0].lines.len(), DIFF_PREVIEW_LINES);
        assert_eq!(list.files[0].lines[0].kind, "hunk");
        assert_eq!(list.files[0].lines[1].kind, "context");
        assert_eq!(
            (list.files[0].lines[1].old_no, list.files[0].lines[1].new_no),
            (Some(1), Some(1))
        );
        let small = &list.files[1];
        assert_eq!(
            small.lines.iter().map(|l| l.kind).collect::<Vec<_>>(),
            ["hunk", "del", "add"]
        );
        assert_eq!(
            (small.lines[1].old_no, small.lines[1].new_no),
            (Some(1), None)
        );
        let full = diff_response(&diff, Some("a.rs")).unwrap();
        assert_eq!(full.files.len(), 1);
        assert_eq!(full.files[0].lines.len(), 301);
        assert!(!full.truncated);
        assert!(diff_response(&diff, Some("zzz")).is_err());
        let json = serde_json::to_value(&list).unwrap();
        assert!(json["files"][1]["lines"][1].get("newNo").is_none());
    }

    #[test]
    fn checks_and_decisions_summarise_for_the_phone() {
        let check = |name: &str, state: &str| Check {
            name: name.into(),
            state: state.into(),
            ..Check::default()
        };
        let mut timed = check("build", "pass");
        timed.workflow = "CI".into();
        timed.started_at = Some("2026-10-01T10:00:00Z".into());
        timed.completed_at = Some("2026-10-01T10:01:30Z".into());
        let all = |checks: Vec<Check>| Checks {
            head_oid: String::new(),
            checks,
        };
        assert_eq!(
            checks_summary(&all(vec![timed.clone(), check("x", "skipping")])),
            Some("passing")
        );
        assert_eq!(
            checks_summary(&all(vec![timed.clone(), check("y", "fail")])),
            Some("failing")
        );
        assert_eq!(
            checks_summary(&all(vec![timed.clone(), check("y", "pending")])),
            Some("pending")
        );
        assert_eq!(checks_summary(&all(vec![])), None);
        let response = checks_response(&all(vec![timed, check("z", "cancel")]));
        assert_eq!(response.checks[0].name, "CI / build");
        assert_eq!(response.checks[0].duration_ms, Some(90_000));
        assert_eq!(response.checks[1].state, "skipped");
        for (raw, wanted) in [
            ("APPROVED", Some("approved")),
            ("CHANGES_REQUESTED", Some("changesRequested")),
            ("REVIEW_REQUIRED", Some("pending")),
            ("", None),
        ] {
            let details = Details {
                review_decision: raw.into(),
                ..Details::default()
            };
            assert_eq!(review_decision(&details), wanted);
        }
    }

    #[test]
    fn a_thread_merges_comments_replies_and_commits_in_time_order() {
        let comment = |id: &str, at: &str, kind: &str, replies: Vec<Comment>| Comment {
            id: id.into(),
            kind: kind.into(),
            author: "ana".into(),
            body: format!("body {id}"),
            created_at: at.into(),
            replies,
            ..Comment::default()
        };
        let thread = Thread {
            comments: vec![
                comment(
                    "c1",
                    "2026-10-01T10:00:00Z",
                    "comment",
                    vec![comment(
                        "r1",
                        "2026-10-01T12:00:00Z",
                        "review_comment",
                        vec![],
                    )],
                ),
                comment("rv", "2026-10-01T09:00:00Z", "review", vec![]),
            ],
            commits: vec![Commit {
                oid: "abc".into(),
                message_headline: "Fix it".into(),
                author: "sam".into(),
                committed_date: "2026-10-01T11:00:00Z".into(),
                url: String::new(),
            }],
            ..Thread::default()
        };
        let entries = thread_entries(&item(1, 1, ""), &thread, &AvatarBook::default());
        let order: Vec<_> = entries.iter().map(|e| (e.id.as_str(), e.kind)).collect();
        assert_eq!(
            order,
            [
                ("rv", "review"),
                ("c1", "comment"),
                ("abc", "commit"),
                ("r1", "comment")
            ]
        );
        let (seen, triage, projects) = (Seen::default(), Triage::default(), projects());
        let view = view(&seen, &triage, &projects);
        let response = item_response(
            &item(1, 1, ""),
            &view,
            &AvatarBook::default(),
            None,
            Some(&thread),
            None,
        );
        assert_eq!(response.item.comments, 3);
    }

    #[test]
    fn image_urls_come_back_in_reading_order() {
        let body = "Intro ![a](https://x.test/1.png) text <img width=\"3\" src=\"https://x.test/2.png\"> ![b](<https://x.test/3.png> \"t\")";
        assert_eq!(
            image_urls(body),
            [
                "https://x.test/1.png",
                "https://x.test/2.png",
                "https://x.test/3.png"
            ]
        );
        assert!(image_urls("no images, just [a link](https://x.test)").is_empty());
        assert!(media_response("image/png", &vec![0; MAX_IMAGE_BYTES + 1]).is_err());
        assert_eq!(media_response("image/png", b"hi").unwrap().base64, "aGk=");
    }

    #[test]
    fn provider_errors_lose_anything_shaped_like_a_credential() {
        let raw = "HTTP 401: Bad credentials ghp_abcdefghijklmnopqrstuvwxyz0123456789 for https://user:pw@host/x\nsecond line";
        let safe = safe_error(raw);
        assert!(!safe.contains("ghp_") && !safe.contains("pw@") && !safe.contains("second"));
        assert!(safe.starts_with("HTTP 401: Bad credentials"));
        assert!(safe_error(&"word ".repeat(100)).chars().count() <= MAX_ERROR_CHARS + 1);
        assert_eq!(safe_error("Authorization: Bearer abc"), "Authorization: …");
        assert_eq!(
            safe_error("read /Users/me/linear-token failed"),
            "read … failed"
        );
    }

    /// Keys and values that would mean a credential or a config crossed the link.
    fn assert_clean(value: &Value, path: &str) {
        const FORBIDDEN_KEYS: [&str; 9] = [
            "token",
            "secret",
            "password",
            "authorization",
            "apikey",
            "api_key",
            "config",
            "email",
            "credential",
        ];
        const FORBIDDEN_VALUES: [&str; 8] = [
            "ghp_",
            "gho_",
            "github_pat_",
            "glpat-",
            "lin_api_",
            "linear-token",
            "-config.json",
            "Bearer ",
        ];
        match value {
            Value::Object(map) => {
                for (key, inner) in map {
                    let lower = key.to_lowercase();
                    assert!(
                        !FORBIDDEN_KEYS.iter().any(|bad| lower.contains(bad)),
                        "field {path}.{key} could carry a credential"
                    );
                    assert_clean(inner, &format!("{path}.{key}"));
                }
            }
            Value::Array(items) => items.iter().for_each(|inner| assert_clean(inner, path)),
            Value::String(text) => assert!(
                !FORBIDDEN_VALUES.iter().any(|bad| text.contains(bad)),
                "{path} holds credential-shaped text: {text}"
            ),
            _ => {}
        }
    }

    #[test]
    fn no_response_type_carries_a_token_or_config() {
        let secret_error = "401 Bad credentials ghp_abcdefghijklmnopqrstuvwxyz0123456789 see /Users/me/linear-token and jira-config.json Authorization: Bearer abc123";
        let mut items = vec![item(1, 1, "review_requested")];
        items[0].author.avatar_url = "https://avatars.example/a.png".into();
        let (seen, triage, projects) = (Seen::default(), Triage::default(), projects());
        let view = view(&seen, &triage, &projects);
        let avatars = AvatarBook::default();
        let mut connections = BTreeMap::new();
        connections.insert(
            "jira".to_string(),
            Status {
                connected: true,
                url: "https://acme.atlassian.net".into(),
                email: "me@acme.test".into(),
                site: "acme".into(),
                ..Status::default()
            },
        );
        let mut errors = BTreeMap::new();
        errors.insert("github".to_string(), secret_error.to_string());
        errors.insert("jira".to_string(), secret_error.to_string());
        let state = SnapshotState {
            fetched_at: Some(NOW),
            interval_ms: 1000,
            connections: &connections,
            errors: &errors,
        };
        let list = list_response(&items, &params(json!({})), &view, &avatars, &state).unwrap();
        let details = Details {
            body: "Body".into(),
            author: "sam".into(),
            ..Details::default()
        };
        let thread = Thread {
            comments: vec![Comment {
                id: "c".into(),
                author: "ana".into(),
                body: "hi".into(),
                created_at: "2026-10-01T10:00:00Z".into(),
                ..Comment::default()
            }],
            ..Thread::default()
        };
        let checks = Checks {
            head_oid: "abc".into(),
            checks: vec![Check {
                name: "build".into(),
                state: "pass".into(),
                url: Some("https://github.com/acme/app/actions/runs/1".into()),
                ..Check::default()
            }],
        };
        let responses = vec![
            serde_json::to_value(&list).unwrap(),
            serde_json::to_value(item_response(
                &items[0],
                &view,
                &avatars,
                Some(&details),
                Some(&thread),
                Some(&checks),
            ))
            .unwrap(),
            serde_json::to_value(diff_response(&sample_diff(), None).unwrap()).unwrap(),
            serde_json::to_value(checks_response(&checks)).unwrap(),
            serde_json::to_value(media_response("image/png", b"png").unwrap()).unwrap(),
            serde_json::to_value(slim_item(&items[0], &view, &avatars)).unwrap(),
        ];
        for response in &responses {
            assert_clean(response, "$");
        }
        let serialized = serde_json::to_string(&responses).unwrap();
        assert!(!serialized.contains("me@acme.test") && !serialized.contains("atlassian"));
        assert!(!list
            .providers
            .iter()
            .any(|p| p.error.as_deref().is_some_and(|e| e.contains("ghp_"))));
    }
}
