//! Port of src/features/inbox/ui/InboxFiltersMenu.tsx: the list's filter
//! popover. Assignment, status, time, and type for every source; Linear
//! teams and projects, Jira projects, or rail projects for the sources that
//! have them; and "Clear filters".

use gpui::{
    AnyElement, App, ElementId, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div,
};
use monocode_ui::widgets::popover_frame;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use crate::data::{
    Action, InboxFilters, InboxKind, InboxListState, InboxProjectOption, InboxProvider,
    InboxSource, InboxTimeFilter, ValueAction,
};
use crate::model::is_tracker_source;
use crate::style::{rule, section_label};

/// `INBOX_FILTER_MENU_WIDTH`.
pub const INBOX_FILTER_MENU_WIDTH: f32 = 228.;

/// One status box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusKey {
    Open,
    Draft,
    Closed,
    Merged,
}

/// What a menu row does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterAction {
    AssignedToMe,
    Status(StatusKey),
    Time(InboxTimeFilter),
    Kind(InboxKind),
    LinearTeam(String),
    LinearProject(String),
    JiraProject(String),
    Project(String),
    Clear,
}

/// One row of the menu.
#[derive(Debug, Clone, PartialEq)]
pub enum FilterEntry {
    Section(&'static str),
    Item {
        action: FilterAction,
        label: String,
        checked: bool,
        icon: Option<FilterIcon>,
    },
    Separator,
}

/// The icon a row leads with.
#[derive(Debug, Clone, PartialEq)]
pub enum FilterIcon {
    Glyph(IconName),
    Project(InboxProjectOption),
}

const TIME_OPTIONS: [(InboxTimeFilter, &str); 4] = [
    (InboxTimeFilter::All, "All time"),
    (InboxTimeFilter::Today, "Today"),
    (InboxTimeFilter::SevenDays, "Last 7 days"),
    (InboxTimeFilter::ThirtyDays, "Last 30 days"),
];

fn item(action: FilterAction, label: impl Into<String>, checked: bool) -> FilterEntry {
    FilterEntry::Item {
        action,
        label: label.into(),
        checked,
        icon: None,
    }
}

/// The rows the menu shows for this source and these filters.
pub fn filter_entries(state: &InboxListState) -> Vec<FilterEntry> {
    let source = state.source;
    let filters = &state.filters;
    let tracker = is_tracker_source(source);
    let repository_attention = matches!(source, InboxProvider::Gitlab | InboxProvider::AzureDevops);
    let mut entries = vec![item(
        FilterAction::AssignedToMe,
        if repository_attention {
            "Needs attention"
        } else {
            "Assigned to me"
        },
        filters.assigned_to_me,
    )];
    entries.push(FilterEntry::Section("Status"));
    entries.push(item(
        FilterAction::Status(StatusKey::Open),
        "Open",
        filters.status.open,
    ));
    if !tracker {
        entries.push(item(
            FilterAction::Status(StatusKey::Draft),
            "Draft",
            filters.status.draft,
        ));
    }
    entries.push(item(
        FilterAction::Status(StatusKey::Closed),
        "Closed",
        filters.status.closed,
    ));
    if !tracker {
        entries.push(item(
            FilterAction::Status(StatusKey::Merged),
            "Merged",
            filters.status.merged,
        ));
    }
    entries.push(FilterEntry::Section("Time"));
    for (time, label) in TIME_OPTIONS {
        entries.push(item(FilterAction::Time(time), label, filters.time == time));
    }
    if !tracker {
        entries.push(FilterEntry::Section("Type"));
        for (kind, label, glyph) in [
            (InboxKind::Issue, "Issues", IconName::CircleDot),
            (InboxKind::Pr, "Pull requests", IconName::GitPullRequest),
        ] {
            let label = if source == InboxProvider::Gitlab && kind == InboxKind::Pr {
                "Merge requests"
            } else {
                label
            };
            entries.push(FilterEntry::Item {
                action: FilterAction::Kind(kind),
                label: label.into(),
                checked: !filters.hidden_kinds.contains(&kind),
                icon: Some(FilterIcon::Glyph(glyph)),
            });
        }
    }
    if source == InboxProvider::Linear && !state.linear_teams.is_empty() {
        entries.push(FilterEntry::Section("Teams"));
        for team in &state.linear_teams {
            entries.push(item(
                FilterAction::LinearTeam(team.id.clone()),
                if team.name.is_empty() {
                    team.key.clone()
                } else {
                    team.name.clone()
                },
                !state.hidden_linear_team_ids.contains(&team.id),
            ));
        }
    }
    if source == InboxProvider::Linear && !state.linear_projects.is_empty() {
        entries.push(FilterEntry::Section("Projects"));
        for project in &state.linear_projects {
            entries.push(item(
                FilterAction::LinearProject(project.id.clone()),
                project.name.clone(),
                !filters.hidden_linear_projects.contains(&project.id),
            ));
        }
    }
    if source == InboxProvider::Jira && !state.jira_projects.is_empty() {
        entries.push(FilterEntry::Section("Projects"));
        for project in &state.jira_projects {
            entries.push(item(
                FilterAction::JiraProject(project.id.clone()),
                if project.name.is_empty() {
                    project.key.clone()
                } else {
                    project.name.clone()
                },
                !state.hidden_jira_project_ids.contains(&project.id),
            ));
        }
    }
    if !tracker && !(repository_attention && filters.assigned_to_me) && !state.projects.is_empty() {
        entries.push(FilterEntry::Section("Projects"));
        for project in &state.projects {
            entries.push(FilterEntry::Item {
                action: FilterAction::Project(project.path.clone()),
                label: project.name.clone(),
                checked: !filters.hidden_projects.contains(&project.path),
                icon: project
                    .mark
                    .logo_path
                    .is_some()
                    .then(|| FilterIcon::Project(project.clone())),
            });
        }
    }
    if state.filters_active {
        entries.push(FilterEntry::Separator);
        entries.push(item(FilterAction::Clear, "Clear filters", false));
    }
    entries
}

/// What a row changes.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FilterChange {
    pub filters: Option<InboxFilters>,
    pub hidden_linear_team_ids: Option<Vec<String>>,
    pub hidden_jira_project_ids: Option<Vec<String>>,
}

fn toggled(list: &[String], value: &str) -> Vec<String> {
    if list.iter().any(|entry| entry == value) {
        list.iter()
            .filter(|entry| *entry != value)
            .cloned()
            .collect()
    } else {
        let mut next = list.to_vec();
        next.push(value.to_string());
        next
    }
}

/// The toggles of `InboxFiltersMenu`.
pub fn apply_filter_action(
    action: &FilterAction,
    source: InboxSource,
    state: &InboxListState,
) -> FilterChange {
    let filters = &state.filters;
    let mut next = filters.clone();
    match action {
        FilterAction::AssignedToMe => next.assigned_to_me = !filters.assigned_to_me,
        FilterAction::Status(key) => {
            let status = &mut next.status;
            match key {
                StatusKey::Open => status.open = !status.open,
                StatusKey::Draft => status.draft = !status.draft,
                StatusKey::Closed => status.closed = !status.closed,
                StatusKey::Merged => status.merged = !status.merged,
            }
        }
        FilterAction::Time(time) => next.time = *time,
        FilterAction::Kind(kind) => {
            if next.hidden_kinds.contains(kind) {
                next.hidden_kinds.retain(|entry| entry != kind);
            } else {
                next.hidden_kinds.push(*kind);
            }
        }
        FilterAction::Project(path) => {
            next.hidden_projects = toggled(&filters.hidden_projects, path)
        }
        FilterAction::LinearProject(id) => {
            next.hidden_linear_projects = toggled(&filters.hidden_linear_projects, id)
        }
        FilterAction::LinearTeam(id) => {
            return FilterChange {
                hidden_linear_team_ids: Some(toggled(&state.hidden_linear_team_ids, id)),
                ..Default::default()
            };
        }
        FilterAction::JiraProject(id) => {
            return FilterChange {
                hidden_jira_project_ids: Some(toggled(&state.hidden_jira_project_ids, id)),
                ..Default::default()
            };
        }
        FilterAction::Clear => {
            let teams_active =
                source == InboxProvider::Linear && !state.hidden_linear_team_ids.is_empty();
            let jira_active =
                source == InboxProvider::Jira && !state.hidden_jira_project_ids.is_empty();
            return FilterChange {
                filters: Some(InboxFilters::default()),
                hidden_linear_team_ids: teams_active.then(Vec::new),
                hidden_jira_project_ids: jira_active.then(Vec::new),
            };
        }
    }
    FilterChange {
        filters: Some(next),
        ..Default::default()
    }
}

/// Renders the menu rows inside a popover frame. `on_action` runs for a
/// picked row; `on_dismiss` for a click outside.
pub fn inbox_filters_menu(
    entries: Vec<FilterEntry>,
    animate: bool,
    project_mark: impl Fn(&InboxProjectOption, &App) -> AnyElement,
    on_action: ValueAction<FilterAction>,
    on_dismiss: Action,
    cx: &App,
) -> AnyElement {
    let theme = Theme::of(cx);
    let mut list = div()
        .id("inbox-filters-list")
        .flex()
        .flex_col()
        .max_h(u(480.))
        .overflow_y_scroll()
        .p(u(4.))
        .on_mouse_down_out(move |_, window, cx| on_dismiss(window, cx));
    for (index, entry) in entries.into_iter().enumerate() {
        match entry {
            FilterEntry::Section(label) => list = list.child(section_label(label, cx)),
            FilterEntry::Separator => {
                list = list.child(div().my(u(4.)).child(rule(cx)));
            }
            FilterEntry::Item {
                action,
                label,
                checked,
                icon: lead,
            } => {
                let clear = action == FilterAction::Clear;
                let hover = theme.content(0.05);
                let ink = theme.colors.content;
                let pick = on_action.clone();
                let mut row = div()
                    .id(ElementId::NamedInteger("inbox-filter".into(), index as u64))
                    .flex()
                    .h(u(28.))
                    .w_full()
                    .items_center()
                    .gap(u(8.))
                    .rounded(u(theme.radius.lg))
                    .px(u(8.))
                    .text_px(theme.text.body)
                    .leading(theme.leading.none)
                    .hover(move |s| s.bg(hover).text_color(ink))
                    .on_click(move |_, window, cx| pick(action.clone(), window, cx));
                row = row.text_color(if clear { theme.content(0.70) } else { ink });
                match lead {
                    Some(FilterIcon::Glyph(name)) => {
                        row = row.child(icon(name).size(u(14.)).text_color(ink));
                    }
                    Some(FilterIcon::Project(project)) => {
                        row = row.child(project_mark(&project, cx));
                    }
                    None => {}
                }
                row = row.child(div().flex_1().min_w_0().truncate().child(label));
                if checked {
                    row = row.child(icon(IconName::Check).size(u(14.)).text_color(ink));
                }
                list = list.child(row);
            }
        }
    }
    popover_frame("inbox-filters")
        .width(INBOX_FILTER_MENU_WIDTH)
        .max_height(480.)
        .animate(animate)
        .child(list)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(entries: &[FilterEntry]) -> Vec<String> {
        entries
            .iter()
            .map(|entry| match entry {
                FilterEntry::Section(label) => format!("[{label}]"),
                FilterEntry::Item { label, .. } => label.clone(),
                FilterEntry::Separator => "---".into(),
            })
            .collect()
    }

    #[test]
    fn gitlab_names_attention_and_merge_requests() {
        let state = InboxListState {
            source: InboxProvider::Gitlab,
            ..Default::default()
        };
        let labels = labels(&filter_entries(&state));
        assert_eq!(labels[0], "Needs attention");
        assert!(labels.contains(&"Merge requests".to_string()));
        assert!(labels.contains(&"Draft".to_string()));
    }

    #[test]
    fn trackers_have_no_draft_merged_or_type_rows() {
        let state = InboxListState {
            source: InboxProvider::Linear,
            ..Default::default()
        };
        let labels = labels(&filter_entries(&state));
        assert!(!labels.contains(&"Draft".to_string()));
        assert!(!labels.contains(&"Merged".to_string()));
        assert!(!labels.contains(&"[Type]".to_string()));
    }

    #[test]
    fn clear_filters_resets_shared_rosters_only_for_the_open_tab() {
        let state = InboxListState {
            source: InboxProvider::Linear,
            filters_active: true,
            hidden_linear_team_ids: vec!["team".into()],
            hidden_jira_project_ids: vec!["jira".into()],
            filters: InboxFilters {
                assigned_to_me: true,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(labels(&filter_entries(&state)).contains(&"Clear filters".to_string()));
        let change = apply_filter_action(&FilterAction::Clear, InboxProvider::Linear, &state);
        assert_eq!(change.filters, Some(InboxFilters::default()));
        assert_eq!(change.hidden_linear_team_ids, Some(Vec::new()));
        assert_eq!(change.hidden_jira_project_ids, None);
    }

    #[test]
    fn toggles_kinds_and_projects() {
        let state = InboxListState::default();
        let change = apply_filter_action(
            &FilterAction::Kind(InboxKind::Pr),
            InboxProvider::Github,
            &state,
        );
        assert_eq!(change.filters.unwrap().hidden_kinds, [InboxKind::Pr]);
        let change = apply_filter_action(
            &FilterAction::Project("/tmp/web".into()),
            InboxProvider::Github,
            &state,
        );
        assert_eq!(change.filters.unwrap().hidden_projects, ["/tmp/web"]);
    }
}
