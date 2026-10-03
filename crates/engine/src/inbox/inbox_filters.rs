//! Port of src/features/inbox/model/inboxFilters.ts: the filter state, the
//! source tabs and their connection cache, and the filter functions.
//!
//! localStorage becomes `Kv` with the same keys and JSON.

use std::collections::HashSet;

use monocode_settings::Kv;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::github_tasks::filter_inbox_items;
use super::text::{locale_compare, non_empty_trimmed};
use super::time::{InboxTimeFilter, date_parse, time_filter_start};
use super::types::{InboxItem, InboxKind, InboxProvider, InboxState, provider_str};
use crate::runtime::util::project_path::normalize_project_path;

/// `InboxStatusFilter`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct InboxStatusFilter {
    pub open: bool,
    pub draft: bool,
    pub closed: bool,
    pub merged: bool,
}

/// `InboxFilters`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxFilters {
    pub assigned_to_me: bool,
    pub hidden_projects: Vec<String>,
    /// Linear project ids to hide. `LINEAR_NO_PROJECT` stands for issues
    /// outside every project.
    pub hidden_linear_projects: Vec<String>,
    pub hidden_kinds: Vec<InboxKind>,
    pub time: InboxTimeFilter,
    pub status: InboxStatusFilter,
}

/// `DEFAULT_INBOX_FILTERS`.
pub fn default_inbox_filters() -> InboxFilters {
    InboxFilters::default()
}

/// Stands in for "issue belongs to no Linear project" so that bucket is as
/// hideable as a real one. Not a valid Linear id, so it never collides.
pub const LINEAR_NO_PROJECT: &str = "~none";

/// `LinearProjectOption`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinearProjectOption {
    pub id: String,
    pub name: String,
}

/// `InboxSource`, the provider tabs.
pub type InboxSource = InboxProvider;

/// `InboxSourceConnections`: `None` means the status check has not
/// resolved yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct InboxSourceConnections {
    pub github: Option<bool>,
    pub linear: Option<bool>,
    pub jira: Option<bool>,
    pub gitlab: Option<bool>,
    pub azuredevops: Option<bool>,
}

impl InboxSourceConnections {
    pub fn get(&self, source: InboxSource) -> Option<bool> {
        match source {
            InboxProvider::Github => self.github,
            InboxProvider::Linear => self.linear,
            InboxProvider::Jira => self.jira,
            InboxProvider::Gitlab => self.gitlab,
            InboxProvider::AzureDevops => self.azuredevops,
        }
    }

    pub fn set(&mut self, source: InboxSource, value: Option<bool>) {
        match source {
            InboxProvider::Github => self.github = value,
            InboxProvider::Linear => self.linear = value,
            InboxProvider::Jira => self.jira = value,
            InboxProvider::Gitlab => self.gitlab = value,
            InboxProvider::AzureDevops => self.azuredevops = value,
        }
    }
}

/// The tab order.
const SOURCE_ORDER: [InboxSource; 5] = [
    InboxProvider::Github,
    InboxProvider::Linear,
    InboxProvider::Jira,
    InboxProvider::Gitlab,
    InboxProvider::AzureDevops,
];

/// `INBOX_SOURCE_LABELS`.
pub fn inbox_source_label(source: InboxSource) -> &'static str {
    match source {
        InboxProvider::Github => "GitHub",
        InboxProvider::Linear => "Linear",
        InboxProvider::Jira => "Jira",
        InboxProvider::Gitlab => "GitLab",
        InboxProvider::AzureDevops => "ADO",
    }
}

/// `visibleInboxSources`: every source not known to be disconnected.
pub fn visible_inbox_sources(connections: &InboxSourceConnections) -> Vec<InboxSource> {
    SOURCE_ORDER
        .into_iter()
        .filter(|source| connections.get(*source) != Some(false))
        .collect()
}

/// `connectableInboxSources`: the sources confirmed to be disconnected.
pub fn connectable_inbox_sources(connections: &InboxSourceConnections) -> Vec<InboxSource> {
    SOURCE_ORDER
        .into_iter()
        .filter(|source| connections.get(*source) == Some(false))
        .collect()
}

/// `isTrackerSource`: account-wide issue trackers have no local repos, no
/// PRs, and no draft or merged states.
pub fn is_tracker_source(source: Option<InboxSource>) -> bool {
    matches!(source, Some(InboxProvider::Linear | InboxProvider::Jira))
}

/// `resolveInboxSource`.
pub fn resolve_inbox_source(
    source: InboxSource,
    connections: &InboxSourceConnections,
) -> InboxSource {
    let visible = visible_inbox_sources(connections);
    if visible.contains(&source) {
        source
    } else {
        visible.first().copied().unwrap_or(InboxProvider::Github)
    }
}

/// `FILTERS_KEY`.
pub const INBOX_FILTERS_KEY: &str = "monocode.inboxFilters";
/// `SOURCE_KEY`.
pub const INBOX_SOURCE_KEY: &str = "monocode.inboxSource";
/// `CONNECTIONS_KEY`.
pub const INBOX_CONNECTIONS_KEY: &str = "monocode.inboxConnections";

/// `loadInboxSource`.
pub fn load_inbox_source(kv: &Kv) -> InboxSource {
    match kv.get_item(INBOX_SOURCE_KEY).as_deref() {
        Some("linear") => InboxProvider::Linear,
        Some("jira") => InboxProvider::Jira,
        Some("gitlab") => InboxProvider::Gitlab,
        Some("azuredevops") => InboxProvider::AzureDevops,
        _ => InboxProvider::Github,
    }
}

/// `saveInboxSource`.
pub fn save_inbox_source(kv: &Kv, source: InboxSource) {
    kv.set_item(INBOX_SOURCE_KEY, provider_str(source));
}

/// `loadInboxConnections`: seeded from the last known answer so a
/// returning user does not watch every tab paint and then drop two.
pub fn load_inbox_connections(kv: &Kv) -> InboxSourceConnections {
    let Some(raw) = kv
        .get_item(INBOX_CONNECTIONS_KEY)
        .filter(|raw| !raw.is_empty())
    else {
        return InboxSourceConnections::default();
    };
    let Ok(Value::Object(record)) = serde_json::from_str::<Value>(&raw) else {
        return InboxSourceConnections::default();
    };
    let flag = |key: &str| record.get(key).and_then(Value::as_bool);
    InboxSourceConnections {
        github: flag("github"),
        linear: flag("linear"),
        jira: flag("jira"),
        gitlab: flag("gitlab"),
        azuredevops: flag("azuredevops"),
    }
}

/// `saveInboxConnections`.
pub fn save_inbox_connections(kv: &Kv, connections: &InboxSourceConnections) {
    if let Ok(json) = serde_json::to_string(connections) {
        kv.set_item(INBOX_CONNECTIONS_KEY, &json);
    }
}

fn string_list(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// `loadInboxFilters`.
pub fn load_inbox_filters(kv: &Kv) -> InboxFilters {
    let Some(raw) = kv.get_item(INBOX_FILTERS_KEY).filter(|raw| !raw.is_empty()) else {
        return InboxFilters::default();
    };
    let Ok(parsed) = serde_json::from_str::<Value>(&raw) else {
        return InboxFilters::default();
    };
    // `JSON.parse("null").assignedToMe` throws, which falls back to the
    // defaults.
    if parsed.is_null() {
        return InboxFilters::default();
    }
    let status = parsed.get("status");
    let status_flag = |key: &str| {
        status
            .and_then(|status| status.get(key))
            .and_then(Value::as_bool)
            == Some(true)
    };
    InboxFilters {
        assigned_to_me: parsed.get("assignedToMe").and_then(Value::as_bool) == Some(true),
        hidden_projects: string_list(parsed.get("hiddenProjects")),
        hidden_linear_projects: string_list(parsed.get("hiddenLinearProjects")),
        hidden_kinds: parsed
            .get("hiddenKinds")
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(|value| match value.as_str() {
                        Some("issue") => Some(InboxKind::Issue),
                        Some("pr") => Some(InboxKind::Pr),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default(),
        time: parsed
            .get("time")
            .and_then(Value::as_str)
            .and_then(InboxTimeFilter::parse)
            .unwrap_or_default(),
        status: InboxStatusFilter {
            open: status_flag("open"),
            draft: status_flag("draft"),
            closed: status_flag("closed"),
            merged: status_flag("merged"),
        },
    }
}

/// `saveInboxFilters`.
pub fn save_inbox_filters(kv: &Kv, filters: &InboxFilters) {
    if let Ok(json) = serde_json::to_string(filters) {
        kv.set_item(INBOX_FILTERS_KEY, &json);
    }
}

/// `pruneInboxFilters`: drop hidden projects that left the rail.
pub fn prune_inbox_filters(filters: &InboxFilters, project_paths: &[String]) -> InboxFilters {
    let known: HashSet<String> = project_paths
        .iter()
        .map(|path| normalize_project_path(path))
        .collect();
    let hidden_projects: Vec<String> = filters
        .hidden_projects
        .iter()
        .filter(|path| known.contains(&normalize_project_path(path)))
        .cloned()
        .collect();
    if hidden_projects.len() == filters.hidden_projects.len() {
        return filters.clone();
    }
    InboxFilters {
        hidden_projects,
        ..filters.clone()
    }
}

/// `hasActiveInboxFilters`. Linear teams and Jira projects live outside
/// `InboxFilters`: they narrow the fetch and are shared with Settings.
pub fn has_active_inbox_filters(
    filters: &InboxFilters,
    source: Option<InboxSource>,
    hidden_linear_team_ids: &[String],
    hidden_jira_project_ids: &[String],
) -> bool {
    let status = &filters.status;
    let status_active = if is_tracker_source(source) {
        status.open || status.closed
    } else {
        status.open || status.draft || status.closed || status.merged
    };
    let hidden_scope = if source == Some(InboxProvider::Linear) {
        !filters.hidden_linear_projects.is_empty()
    } else {
        source != Some(InboxProvider::Jira) && !filters.hidden_projects.is_empty()
    };
    filters.assigned_to_me
        || (source == Some(InboxProvider::Linear) && !hidden_linear_team_ids.is_empty())
        || (source == Some(InboxProvider::Jira) && !hidden_jira_project_ids.is_empty())
        || hidden_scope
        || (!is_tracker_source(source) && !filters.hidden_kinds.is_empty())
        || filters.time != InboxTimeFilter::All
        || status_active
}

/// `inboxFetchState`: no status box checked means "no restriction", so the
/// fetch has to widen with it.
pub fn inbox_fetch_state(filters: &InboxFilters) -> InboxState {
    let InboxStatusFilter {
        open,
        draft,
        closed,
        merged,
    } = filters.status;
    if closed || merged {
        return InboxState::All;
    }
    if open || draft {
        InboxState::Open
    } else {
        InboxState::All
    }
}

/// `filterInboxByProject`.
pub fn filter_inbox_by_project(items: &[InboxItem], hidden_projects: &[String]) -> Vec<InboxItem> {
    let hidden: HashSet<String> = hidden_projects
        .iter()
        .map(|path| normalize_project_path(path))
        .collect();
    if hidden.is_empty() {
        return items.to_vec();
    }
    items
        .iter()
        .filter(|item| {
            let path = normalize_project_path(&item.project_path);
            path.is_empty() || !hidden.contains(&path)
        })
        .cloned()
        .collect()
}

/// `linearProjectOptions`: every distinct Linear project in `items`,
/// name-sorted, with a trailing "No project" row when an issue sits outside
/// every project.
pub fn linear_project_options(items: &[InboxItem]) -> Vec<LinearProjectOption> {
    let mut by_id: Vec<(String, String)> = Vec::new();
    let mut unassigned = false;
    for item in items {
        if item.provider != InboxProvider::Linear {
            continue;
        }
        let id = item
            .project_id
            .as_deref()
            .map(monocode_core::js::trim)
            .unwrap_or("");
        if id.is_empty() {
            unassigned = true;
            continue;
        }
        if !by_id.iter().any(|(existing, _)| existing == id) {
            let name = non_empty_trimmed(item.project_name.as_deref()).unwrap_or(id);
            by_id.push((id.to_string(), name.to_string()));
        }
    }
    let mut options: Vec<LinearProjectOption> = by_id
        .into_iter()
        .map(|(id, name)| LinearProjectOption { id, name })
        .collect();
    options.sort_by(|a, b| locale_compare(&a.name, &b.name));
    if unassigned {
        options.push(LinearProjectOption {
            id: LINEAR_NO_PROJECT.into(),
            name: "No project".into(),
        });
    }
    options
}

/// `filterInboxByLinearProject`.
pub fn filter_inbox_by_linear_project(
    items: &[InboxItem],
    hidden_projects: &[String],
) -> Vec<InboxItem> {
    if hidden_projects.is_empty() {
        return items.to_vec();
    }
    items
        .iter()
        .filter(|item| {
            if item.provider != InboxProvider::Linear {
                return true;
            }
            let id = non_empty_trimmed(item.project_id.as_deref()).unwrap_or(LINEAR_NO_PROJECT);
            !hidden_projects.iter().any(|hidden| hidden == id)
        })
        .cloned()
        .collect()
}

/// `filterInboxByKind`.
pub fn filter_inbox_by_kind(items: &[InboxItem], hidden_kinds: &[InboxKind]) -> Vec<InboxItem> {
    items
        .iter()
        .filter(|item| !hidden_kinds.contains(&item.kind))
        .cloned()
        .collect()
}

/// `filterInboxByStatus`: an item passes when any checked box matches.
pub fn filter_inbox_by_status(items: &[InboxItem], status: &InboxStatusFilter) -> Vec<InboxItem> {
    let any = status.open || status.draft || status.closed || status.merged;
    if !any {
        return items.to_vec();
    }
    items
        .iter()
        .filter(|item| {
            let label = item.status();
            (status.open && label == "Open")
                || (status.draft && label == "Draft")
                || (status.closed && label == "Closed")
                || (status.merged && label == "Merged")
        })
        .cloned()
        .collect()
}

/// `filterInboxByTime`.
pub fn filter_inbox_by_time(
    items: &[InboxItem],
    time: InboxTimeFilter,
    now: i64,
) -> Vec<InboxItem> {
    if time == InboxTimeFilter::All {
        return items.to_vec();
    }
    let start = time_filter_start(time, now);
    items
        .iter()
        .filter(|item| date_parse(&item.updated_at).is_some_and(|updated| updated >= start))
        .cloned()
        .collect()
}

/// `filterInboxByProvider`.
pub fn filter_inbox_by_provider(items: &[InboxItem], source: InboxSource) -> Vec<InboxItem> {
    items
        .iter()
        .filter(|item| item.provider == source)
        .cloned()
        .collect()
}

/// `statusFilterForSource`: trackers have no draft or merged state.
pub fn status_filter_for_source(
    status: &InboxStatusFilter,
    source: Option<InboxSource>,
) -> InboxStatusFilter {
    if !is_tracker_source(source) {
        return *status;
    }
    InboxStatusFilter {
        open: status.open,
        closed: status.closed,
        draft: false,
        merged: false,
    }
}

/// `applyInboxFilters`.
pub fn apply_inbox_filters(
    items: &[InboxItem],
    filters: &InboxFilters,
    query: &str,
    now: i64,
    source: Option<InboxSource>,
) -> Vec<InboxItem> {
    let scoped = match source {
        Some(source) => filter_inbox_by_provider(items, source),
        None => items.to_vec(),
    };
    let skip_projects = is_tracker_source(source)
        || (matches!(
            source,
            Some(InboxProvider::Gitlab | InboxProvider::AzureDevops)
        ) && filters.assigned_to_me);
    let hidden_projects: &[String] = if skip_projects {
        &[]
    } else {
        &filters.hidden_projects
    };
    let hidden_kinds: &[InboxKind] = if is_tracker_source(source) {
        &[]
    } else {
        &filters.hidden_kinds
    };
    let by_project = filter_inbox_by_project(&scoped, hidden_projects);
    let by_linear = filter_inbox_by_linear_project(&by_project, &filters.hidden_linear_projects);
    let by_kind = filter_inbox_by_kind(&by_linear, hidden_kinds);
    let by_time = filter_inbox_by_time(&by_kind, filters.time, now);
    let by_status =
        filter_inbox_by_status(&by_time, &status_filter_for_source(&filters.status, source));
    filter_inbox_items(&by_status, query)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inbox::github_tasks::test_items::{item, with};
    use crate::inbox::time::now_ms;

    fn numbers(items: &[InboxItem]) -> Vec<i64> {
        items.iter().map(|row| row.number).collect()
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn linear(number: i64, project: Option<(&str, &str)>) -> InboxItem {
        with(item(number, "2026-08-27T10:00:00Z"), |row| {
            row.kind = InboxKind::Linear;
            row.provider = InboxProvider::Linear;
            row.project_path = String::new();
            row.project_id = Some(project.map(|(id, _)| id.to_string()).unwrap_or_default());
            row.project_name = Some(
                project
                    .map(|(_, name)| name.to_string())
                    .unwrap_or_default(),
            );
        })
    }

    #[test]
    fn filter_by_project_hides_selected_projects() {
        let rows = vec![
            item(1, "2026-08-27T10:00:00Z"),
            with(item(2, "2026-08-27T10:00:00Z"), |row| {
                row.project_path = "/tmp/docs".into()
            }),
        ];
        assert_eq!(
            numbers(&filter_inbox_by_project(&rows, &strings(&["/tmp/web/"]))),
            [2]
        );
    }

    #[test]
    fn filter_by_project_keeps_linear_issues_that_are_not_tied_to_a_folder() {
        // `normalizeProjectPath("")` is "/", so an empty path is only kept
        // when "/" is not hidden.
        let rows = vec![item(1, "2026-08-27T10:00:00Z"), linear(9, None)];
        assert_eq!(
            numbers(&filter_inbox_by_project(&rows, &strings(&["/tmp/web"]))),
            [9]
        );
    }

    #[test]
    fn linear_project_options_collects_distinct_projects_sorted_by_name() {
        let rows = vec![
            linear(1, Some(("p2", "Onboarding"))),
            linear(2, Some(("p1", "Billing"))),
            linear(3, Some(("p1", "Billing"))),
        ];
        assert_eq!(
            linear_project_options(&rows),
            vec![
                LinearProjectOption {
                    id: "p1".into(),
                    name: "Billing".into()
                },
                LinearProjectOption {
                    id: "p2".into(),
                    name: "Onboarding".into()
                },
            ]
        );
    }

    #[test]
    fn linear_project_options_appends_no_project_and_ignores_github() {
        let rows = vec![linear(1, Some(("p1", "Billing"))), linear(2, None)];
        assert_eq!(
            linear_project_options(&rows),
            vec![
                LinearProjectOption {
                    id: "p1".into(),
                    name: "Billing".into()
                },
                LinearProjectOption {
                    id: LINEAR_NO_PROJECT.into(),
                    name: "No project".into()
                },
            ]
        );
        assert_eq!(
            linear_project_options(&[item(1, "2026-08-27T10:00:00Z")]),
            vec![]
        );
        assert_eq!(
            linear_project_options(&[linear(1, Some(("p1", "")))]),
            vec![LinearProjectOption {
                id: "p1".into(),
                name: "p1".into()
            }]
        );
    }

    #[test]
    fn filter_by_linear_project() {
        let rows = vec![
            linear(1, Some(("p1", "Billing"))),
            linear(2, None),
            item(3, "2026-08-27T10:00:00Z"),
        ];
        assert_eq!(
            numbers(&filter_inbox_by_linear_project(&rows, &[])),
            [1, 2, 3]
        );
        assert_eq!(
            numbers(&filter_inbox_by_linear_project(&rows, &strings(&["p1"]))),
            [2, 3]
        );
        assert_eq!(
            numbers(&filter_inbox_by_linear_project(
                &rows,
                &strings(&[LINEAR_NO_PROJECT])
            )),
            [1, 3]
        );
        assert_eq!(
            numbers(&filter_inbox_by_linear_project(
                &rows,
                &strings(&["p1", LINEAR_NO_PROJECT])
            )),
            [3]
        );
    }

    #[test]
    fn filter_by_kind_hides_selected_kinds() {
        let rows = vec![
            item(1, "2026-08-27T10:00:00Z"),
            with(item(2, "2026-08-27T10:00:00Z"), |row| {
                row.kind = InboxKind::Pr
            }),
            linear(9, None),
        ];
        assert_eq!(
            numbers(&filter_inbox_by_kind(&rows, &[InboxKind::Pr])),
            [1, 9]
        );
        assert_eq!(
            numbers(&filter_inbox_by_kind(&rows, &[InboxKind::Linear])),
            [1, 2]
        );
    }

    #[test]
    fn filter_by_provider_keeps_one_provider() {
        let rows = vec![
            item(1, "2026-08-27T10:00:00Z"),
            with(item(2, "2026-08-27T10:00:00Z"), |row| {
                row.provider = InboxProvider::Gitlab
            }),
            linear(9, None),
        ];
        assert_eq!(
            numbers(&filter_inbox_by_provider(&rows, InboxProvider::Github)),
            [1]
        );
        assert_eq!(
            numbers(&filter_inbox_by_provider(&rows, InboxProvider::Linear)),
            [9]
        );
        assert_eq!(
            numbers(&filter_inbox_by_provider(&rows, InboxProvider::Gitlab)),
            [2]
        );
    }

    fn status_rows() -> Vec<InboxItem> {
        vec![
            item(1, "2026-08-27T10:00:00Z"),
            with(item(2, "2026-08-27T10:00:00Z"), |row| {
                row.kind = InboxKind::Pr;
                row.draft = true;
            }),
            with(item(3, "2026-08-27T10:00:00Z"), |row| {
                row.state = "closed".into()
            }),
            with(item(4, "2026-08-27T10:00:00Z"), |row| {
                row.kind = InboxKind::Pr;
                row.state = "merged".into();
            }),
        ]
    }

    #[test]
    fn filter_by_status() {
        assert_eq!(
            numbers(&filter_inbox_by_status(
                &status_rows(),
                &InboxStatusFilter::default()
            )),
            [1, 2, 3, 4]
        );
        assert_eq!(
            numbers(&filter_inbox_by_status(
                &status_rows(),
                &InboxStatusFilter {
                    open: true,
                    draft: false,
                    closed: true,
                    merged: false
                }
            )),
            [1, 3]
        );
    }

    #[test]
    fn filter_by_time_keeps_items_updated_today() {
        let local = |text: &str| date_parse(text).unwrap();
        let now = local("2026-08-27T15:00:00");
        let rows = vec![
            item(
                1,
                &crate::inbox::time::to_iso_string(local("2026-08-27T10:00:00")),
            ),
            item(
                2,
                &crate::inbox::time::to_iso_string(local("2026-08-20T10:00:00")),
            ),
        ];
        assert_eq!(
            numbers(&filter_inbox_by_time(&rows, InboxTimeFilter::Today, now)),
            [1]
        );
    }

    #[test]
    fn apply_combines_project_kind_and_search_filters() {
        let rows = vec![
            with(item(1, "2026-08-27T10:00:00Z"), |row| {
                row.title = "Fix checkout".into();
                row.kind = InboxKind::Pr;
            }),
            with(item(2, "2026-08-27T10:00:00Z"), |row| {
                row.title = "Fix checkout".into();
                row.project_path = "/tmp/docs".into();
            }),
        ];
        let filters = InboxFilters {
            hidden_projects: strings(&["/tmp/docs"]),
            ..InboxFilters::default()
        };
        assert_eq!(
            numbers(&apply_inbox_filters(
                &rows,
                &filters,
                "checkout",
                now_ms(),
                None
            )),
            [1]
        );
    }

    #[test]
    fn apply_scopes_to_a_provider_tab_and_ignores_github_only_status_on_linear() {
        let rows = vec![
            with(item(1, "2026-08-27T10:00:00Z"), |row| {
                row.title = "Fix checkout".into();
                row.kind = InboxKind::Pr;
            }),
            with(linear(9, None), |row| row.title = "Fix checkout".into()),
        ];
        assert_eq!(
            numbers(&apply_inbox_filters(
                &rows,
                &InboxFilters::default(),
                "checkout",
                now_ms(),
                Some(InboxProvider::Linear)
            )),
            [9]
        );
        let filters = InboxFilters {
            status: InboxStatusFilter {
                open: false,
                draft: true,
                closed: false,
                merged: true,
            },
            ..InboxFilters::default()
        };
        assert_eq!(
            numbers(&apply_inbox_filters(
                &[linear(9, None)],
                &filters,
                "",
                now_ms(),
                Some(InboxProvider::Linear)
            )),
            [9]
        );
    }

    #[test]
    fn apply_ignores_local_project_exclusions_in_gitlabs_attention_view() {
        let gitlab = with(item(9, "2026-08-27T10:00:00Z"), |row| {
            row.provider = InboxProvider::Gitlab
        });
        let filters = InboxFilters {
            assigned_to_me: true,
            hidden_projects: strings(&["/tmp/web"]),
            ..InboxFilters::default()
        };
        assert_eq!(
            apply_inbox_filters(
                std::slice::from_ref(&gitlab),
                &filters,
                "",
                now_ms(),
                Some(InboxProvider::Gitlab)
            ),
            vec![gitlab]
        );
    }

    #[test]
    fn has_active_filters() {
        let defaults = InboxFilters::default();
        assert!(!has_active_inbox_filters(&defaults, None, &[], &[]));
        let hidden = InboxFilters {
            hidden_projects: strings(&["/tmp/web"]),
            ..InboxFilters::default()
        };
        assert!(has_active_inbox_filters(&hidden, None, &[], &[]));
        let github_only = InboxFilters {
            hidden_projects: strings(&["/tmp/web"]),
            hidden_kinds: vec![InboxKind::Pr],
            ..InboxFilters::default()
        };
        assert!(!has_active_inbox_filters(
            &github_only,
            Some(InboxProvider::Linear),
            &[],
            &[]
        ));
        let linear_project = InboxFilters {
            hidden_linear_projects: strings(&["p1"]),
            ..InboxFilters::default()
        };
        assert!(has_active_inbox_filters(
            &linear_project,
            Some(InboxProvider::Linear),
            &[],
            &[]
        ));
        assert!(!has_active_inbox_filters(
            &linear_project,
            Some(InboxProvider::Github),
            &[],
            &[]
        ));
        assert!(has_active_inbox_filters(
            &defaults,
            Some(InboxProvider::Linear),
            &strings(&["t1"]),
            &[]
        ));
        assert!(!has_active_inbox_filters(
            &defaults,
            Some(InboxProvider::Github),
            &strings(&["t1"]),
            &[]
        ));
        assert!(!has_active_inbox_filters(
            &defaults,
            Some(InboxProvider::Linear),
            &[],
            &[]
        ));
    }

    #[test]
    fn fetch_state_widens_with_the_status_boxes() {
        let with_status = |status: InboxStatusFilter| InboxFilters {
            status,
            ..InboxFilters::default()
        };
        assert_eq!(inbox_fetch_state(&InboxFilters::default()), InboxState::All);
        assert_eq!(
            inbox_fetch_state(&with_status(InboxStatusFilter {
                open: true,
                ..Default::default()
            })),
            InboxState::Open
        );
        assert_eq!(
            inbox_fetch_state(&with_status(InboxStatusFilter {
                draft: true,
                ..Default::default()
            })),
            InboxState::Open
        );
        assert_eq!(
            inbox_fetch_state(&with_status(InboxStatusFilter {
                closed: true,
                ..Default::default()
            })),
            InboxState::All
        );
        assert_eq!(
            inbox_fetch_state(&with_status(InboxStatusFilter {
                open: true,
                merged: true,
                ..Default::default()
            })),
            InboxState::All
        );
    }

    #[test]
    fn prune_drops_hidden_projects_that_are_no_longer_in_the_rail() {
        let filters = InboxFilters {
            hidden_projects: strings(&["/tmp/web", "/tmp/gone"]),
            ..InboxFilters::default()
        };
        assert_eq!(
            prune_inbox_filters(&filters, &strings(&["/tmp/web"])).hidden_projects,
            ["/tmp/web"]
        );
    }

    fn connections(values: [Option<bool>; 5]) -> InboxSourceConnections {
        InboxSourceConnections {
            github: values[0],
            linear: values[1],
            jira: values[2],
            gitlab: values[3],
            azuredevops: values[4],
        }
    }

    #[test]
    fn visible_sources_drop_known_disconnected_ones() {
        use InboxProvider::*;
        let f = Some(false);
        let t = Some(true);
        assert_eq!(visible_inbox_sources(&connections([f, f, f, f, f])), vec![]);
        assert_eq!(
            visible_inbox_sources(&connections([t, f, f, f, f])),
            vec![Github]
        );
        assert_eq!(
            visible_inbox_sources(&connections([f, t, t, f, f])),
            vec![Linear, Jira]
        );
        assert_eq!(
            visible_inbox_sources(&connections([t, t, t, t, t])),
            vec![Github, Linear, Jira, Gitlab, AzureDevops]
        );
        assert_eq!(
            visible_inbox_sources(&connections([None; 5])),
            vec![Github, Linear, Jira, Gitlab, AzureDevops]
        );
    }

    #[test]
    fn connectable_sources_are_the_confirmed_disconnected_ones() {
        use InboxProvider::*;
        let f = Some(false);
        let t = Some(true);
        assert_eq!(
            connectable_inbox_sources(&connections([t, f, f, t, t])),
            vec![Linear, Jira]
        );
        assert_eq!(
            connectable_inbox_sources(&connections([f, f, f, f, f])),
            vec![Github, Linear, Jira, Gitlab, AzureDevops]
        );
        assert_eq!(connectable_inbox_sources(&connections([None; 5])), vec![]);
    }

    #[test]
    fn resolve_source_falls_back_to_the_first_visible_one() {
        use InboxProvider::*;
        let f = Some(false);
        let t = Some(true);
        assert_eq!(
            resolve_inbox_source(Linear, &connections([t, f, f, t, f])),
            Github
        );
        assert_eq!(
            resolve_inbox_source(Github, &connections([f, f, f, t, f])),
            Gitlab
        );
        assert_eq!(
            resolve_inbox_source(Linear, &connections([f, f, f, f, f])),
            Github
        );
        assert_eq!(
            resolve_inbox_source(Linear, &connections([f, t, t, f, f])),
            Linear
        );
        assert_eq!(
            resolve_inbox_source(Github, &connections([t, f, f, f, f])),
            Github
        );
    }

    #[test]
    fn connection_cache_round_trips_and_rejects_malformed_values() {
        let kv = Kv::in_memory();
        let saved = connections([Some(true), Some(true), Some(true), Some(false), Some(false)]);
        save_inbox_connections(&kv, &saved);
        assert_eq!(load_inbox_connections(&kv), saved);
        assert_eq!(
            kv.get_item(INBOX_CONNECTIONS_KEY).unwrap(),
            r#"{"github":true,"linear":true,"jira":true,"gitlab":false,"azuredevops":false}"#
        );
        let empty = Kv::in_memory();
        assert_eq!(
            load_inbox_connections(&empty),
            InboxSourceConnections::default()
        );
        empty.set_item(INBOX_CONNECTIONS_KEY, "not json");
        assert_eq!(
            load_inbox_connections(&empty),
            InboxSourceConnections::default()
        );
        empty.set_item(INBOX_CONNECTIONS_KEY, r#"{"linear":"yes"}"#);
        assert_eq!(
            load_inbox_connections(&empty),
            InboxSourceConnections::default()
        );
    }

    #[test]
    fn filters_and_source_round_trip_through_kv() {
        let kv = Kv::in_memory();
        assert_eq!(load_inbox_filters(&kv), InboxFilters::default());
        assert_eq!(load_inbox_source(&kv), InboxProvider::Github);
        let filters = InboxFilters {
            assigned_to_me: true,
            hidden_projects: strings(&["/tmp/web"]),
            hidden_linear_projects: strings(&[LINEAR_NO_PROJECT]),
            hidden_kinds: vec![InboxKind::Pr],
            time: InboxTimeFilter::SevenDays,
            status: InboxStatusFilter {
                open: true,
                ..Default::default()
            },
        };
        save_inbox_filters(&kv, &filters);
        assert_eq!(load_inbox_filters(&kv), filters);
        assert_eq!(
            kv.get_item(INBOX_FILTERS_KEY).unwrap(),
            r#"{"assignedToMe":true,"hiddenProjects":["/tmp/web"],"hiddenLinearProjects":["~none"],"hiddenKinds":["pr"],"time":"7d","status":{"open":true,"draft":false,"closed":false,"merged":false}}"#
        );
        kv.set_item(
            INBOX_FILTERS_KEY,
            r#"{"hiddenKinds":["linear","issue"],"time":"1y","hiddenProjects":["", 4]}"#,
        );
        let loaded = load_inbox_filters(&kv);
        assert_eq!(loaded.hidden_kinds, [InboxKind::Issue]);
        assert_eq!(loaded.time, InboxTimeFilter::All);
        assert!(loaded.hidden_projects.is_empty());
        save_inbox_source(&kv, InboxProvider::AzureDevops);
        assert_eq!(load_inbox_source(&kv), InboxProvider::AzureDevops);
    }
}
