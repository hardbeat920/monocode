//! Port of src/features/inbox/ui/CheckRepairProgress.tsx: which tracked CI
//! repairs belong to a pull request, what each repaired check looks like
//! now, the status chip a check row shows, and one card per repair chat.

use gpui::{
    AnyElement, App, ElementId, Hsla, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, div, prelude::FluentBuilder as _,
};
use monocode_layout::paths::same_project_path;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use crate::data::{
    CiRepairCheck, CiRepairPhase, GithubPrCheck, GithubPrCheckState, GithubPrChecks, PrChecksState,
    TrackedCiRepair,
};
use crate::model::{date_parse, github_actions_job_id};
use crate::style::{active_ink, negative_ink, positive_ink, spin_icon};

/// `RepairState`: a check outcome, or where the repair itself stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RepairState {
    Check(GithubPrCheckState),
    Repairing,
    Waiting,
    Stale,
    Refreshing,
    Stopped,
    Interrupted,
    AgentError,
}

/// One repaired check of one attempt.
#[derive(Debug, Clone, PartialEq)]
pub struct RepairItem {
    pub attempt: TrackedCiRepair,
    pub check: CiRepairCheck,
    pub state: RepairState,
}

/// The repaired checks of one chat.
#[derive(Debug, Clone, PartialEq)]
pub struct RepairGroup {
    pub session_id: String,
    pub items: Vec<RepairItem>,
}

/// `findCheckRepair`: the repair a check row shows. A new commit changes
/// job URLs, so a name match counts only when both sides are unique.
pub fn find_check_repair<'a>(
    groups: &'a [RepairGroup],
    check: &GithubPrCheck,
    current: &GithubPrChecks,
) -> Option<&'a RepairItem> {
    let candidates: Vec<&RepairItem> = groups
        .iter()
        .flat_map(|group| group.items.iter())
        .filter(|item| item.check.name == check.name && item.check.workflow == check.workflow)
        .collect();
    if let Some(exact) = candidates
        .iter()
        .find(|item| item.attempt.head_oid == current.head_oid && item.check.url == check.url)
    {
        return Some(exact);
    }
    if candidates.len() != 1 || candidates[0].attempt.head_oid == current.head_oid {
        return None;
    }
    let same = current
        .checks
        .iter()
        .filter(|item| item.name == check.name && item.workflow == check.workflow)
        .count();
    (same == 1).then_some(candidates[0])
}

fn has_error(view: &PrChecksState) -> bool {
    view.error.as_deref().is_some_and(|error| !error.is_empty())
}

/// `repairState`.
fn repair_state(
    attempt: &TrackedCiRepair,
    check: &CiRepairCheck,
    view: &PrChecksState,
    ambiguous: bool,
) -> RepairState {
    match attempt.phase {
        CiRepairPhase::Running => return RepairState::Repairing,
        CiRepairPhase::Failed => return RepairState::AgentError,
        CiRepairPhase::Cancelled => return RepairState::Stopped,
        CiRepairPhase::Interrupted => return RepairState::Interrupted,
        CiRepairPhase::Completed => {}
    }
    if view.stale || has_error(view) {
        return RepairState::Stale;
    }
    if view.loading || view.refreshing {
        return RepairState::Refreshing;
    }
    let Some(current) = view.checks.as_ref() else {
        return RepairState::Waiting;
    };
    if current.head_oid == attempt.head_oid || ambiguous {
        return RepairState::Waiting;
    }
    let matches: Vec<&GithubPrCheck> = current
        .checks
        .iter()
        .filter(|item| item.name == check.name && item.workflow == check.workflow)
        .collect();
    let originals = attempt
        .checks
        .iter()
        .filter(|item| item.name == check.name && item.workflow == check.workflow)
        .count();
    if matches.len() != 1 || originals != 1 {
        return RepairState::Waiting;
    }
    let latest = matches[0];
    // Only a distinct, newer job can verify a completed repair attempt.
    let started = latest.started_at.as_deref().and_then(date_parse);
    let same_job = github_actions_job_id(check.url.as_deref(), &attempt.repo).is_some()
        && latest.url == check.url;
    match started {
        Some(started) if !same_job && started >= attempt.started_at => {
            RepairState::Check(latest.state)
        }
        _ => RepairState::Waiting,
    }
}

/// `useCheckRepairs`: the repairs of this pull request, one group per chat,
/// each check listed once (the newest attempt on the newest commit wins).
pub fn check_repairs(
    attempts: &[TrackedCiRepair],
    cwd: &str,
    repo: &str,
    number: Option<i64>,
    view: &PrChecksState,
) -> Vec<RepairGroup> {
    struct Seen {
        key: (String, String),
        head_oid: String,
        urls: Vec<Option<String>>,
    }
    let mut seen: Vec<Seen> = Vec::new();
    let mut groups: Vec<RepairGroup> = Vec::new();
    let mut counts: Vec<((String, String), usize)> = Vec::new();
    for attempt in attempts {
        if !same_project_path(&attempt.cwd, cwd)
            || attempt.repo.to_lowercase() != repo.to_lowercase()
            || Some(attempt.number) != number
        {
            continue;
        }
        for check in &attempt.checks {
            let key = (check.workflow.clone(), check.name.clone());
            match seen.iter_mut().find(|entry| entry.key == key) {
                Some(previous) => {
                    if previous.head_oid != attempt.head_oid || previous.urls.contains(&check.url) {
                        continue;
                    }
                    previous.urls.push(check.url.clone());
                }
                None => seen.push(Seen {
                    key: key.clone(),
                    head_oid: attempt.head_oid.clone(),
                    urls: vec![check.url.clone()],
                }),
            }
            let item = RepairItem {
                attempt: attempt.clone(),
                check: check.clone(),
                state: RepairState::Waiting,
            };
            match groups
                .iter_mut()
                .find(|group| group.session_id == attempt.session_id)
            {
                Some(group) => group.items.push(item),
                None => groups.push(RepairGroup {
                    session_id: attempt.session_id.clone(),
                    items: vec![item],
                }),
            }
            match counts.iter_mut().find(|(entry, _)| *entry == key) {
                Some((_, count)) => *count += 1,
                None => counts.push((key, 1)),
            }
        }
    }
    for group in &mut groups {
        for item in &mut group.items {
            let key = (item.check.workflow.clone(), item.check.name.clone());
            let count = counts
                .iter()
                .find(|(entry, _)| *entry == key)
                .map(|(_, count)| *count)
                .unwrap_or(0);
            item.state = repair_state(&item.attempt, &item.check, view, count > 1);
        }
    }
    groups
}

/// The `states` table: the chip label, the summary, the icon, and whether
/// it spins.
pub struct RepairStateLook {
    pub label: &'static str,
    pub summary: &'static str,
    pub icon: IconName,
    pub spins: bool,
}

pub fn repair_look(state: RepairState) -> RepairStateLook {
    use GithubPrCheckState as S;
    let (label, summary, icon, spins) = match state {
        RepairState::Repairing => (
            "Repairing",
            "Repair in progress",
            IconName::LoaderCircle,
            true,
        ),
        RepairState::Waiting => (
            "Awaiting CI",
            "Awaiting new GitHub checks",
            IconName::CircleDashed,
            false,
        ),
        RepairState::Refreshing => (
            "Refreshing",
            "Refreshing GitHub checks",
            IconName::LoaderCircle,
            true,
        ),
        RepairState::Stale => (
            "Out of date",
            "GitHub results are out of date",
            IconName::CircleDashed,
            false,
        ),
        RepairState::Check(S::Pass) => ("CI passed", "passed", IconName::CheckCircle, false),
        RepairState::Check(S::Fail) => ("Still failing", "still failing", IconName::CircleX, false),
        RepairState::Check(S::Pending) => ("CI running", "running", IconName::LoaderCircle, true),
        RepairState::Check(S::Cancel) => {
            ("CI cancelled", "cancelled", IconName::CircleDashed, false)
        }
        RepairState::Check(S::Skipping) => ("CI skipped", "skipped", IconName::CircleDashed, false),
        RepairState::Check(S::Unknown) => ("Unknown", "unknown", IconName::CircleDashed, false),
        RepairState::Stopped => ("Stopped", "Repair stopped", IconName::CircleDashed, false),
        RepairState::Interrupted => (
            "Interrupted",
            "Tracking interrupted",
            IconName::CircleDashed,
            false,
        ),
        RepairState::AgentError => (
            "Agent stopped",
            "Agent could not finish",
            IconName::CircleX,
            false,
        ),
    };
    RepairStateLook {
        label,
        summary,
        icon,
        spins,
    }
}

/// The state's ink: `positive`, `negative`, `active`, or `neutral`.
pub fn repair_ink(state: RepairState, theme: &Theme) -> Hsla {
    use GithubPrCheckState as S;
    match state {
        RepairState::Repairing | RepairState::Check(S::Pending) => active_ink(theme),
        RepairState::Check(S::Pass) => positive_ink(theme),
        RepairState::Check(S::Fail) | RepairState::AgentError => negative_ink(theme),
        _ => theme.content(0.55),
    }
}

fn status_icon(id: ElementId, state: RepairState, theme: &Theme) -> AnyElement {
    let look = repair_look(state);
    let ink = repair_ink(state, theme);
    if look.spins {
        spin_icon(id, look.icon, 14., ink)
    } else {
        icon(look.icon)
            .size(u(14.))
            .text_color(ink)
            .into_any_element()
    }
}

/// `CheckRepairStatus`: the chip a repaired check row shows instead of its
/// state.
pub fn repair_status(id: impl Into<ElementId>, item: &RepairItem, cx: &App) -> AnyElement {
    let theme = Theme::of(cx);
    let look = repair_look(item.state);
    let id: ElementId = id.into();
    div()
        .id(id.clone())
        .flex()
        .flex_none()
        .items_center()
        .gap(u(6.))
        .text_px(theme.text.caption)
        .medium()
        .text_color(repair_ink(item.state, theme))
        .tooltip(monocode_ui::widgets::tooltip(look.summary))
        .child(status_icon(
            ElementId::Name(format!("{id:?}-icon").into()),
            item.state,
            theme,
        ))
        .child(look.label)
        .into_any_element()
}

const PRIORITY: [RepairState; 13] = [
    RepairState::Repairing,
    RepairState::AgentError,
    RepairState::Check(GithubPrCheckState::Fail),
    RepairState::Stale,
    RepairState::Refreshing,
    RepairState::Check(GithubPrCheckState::Pending),
    RepairState::Waiting,
    RepairState::Interrupted,
    RepairState::Stopped,
    RepairState::Check(GithubPrCheckState::Cancel),
    RepairState::Check(GithubPrCheckState::Unknown),
    RepairState::Check(GithubPrCheckState::Skipping),
    RepairState::Check(GithubPrCheckState::Pass),
];

/// What a repair card says: the lead state, one clause per state, and the
/// line under it.
#[derive(Debug, Clone, PartialEq)]
pub struct RepairCardText {
    pub lead: RepairState,
    pub clauses: Vec<(RepairState, String)>,
    pub label: String,
    pub subtitle: String,
    pub subtitle_title: Option<String>,
    /// "Show check" applies: one passing check that is unique on the PR.
    pub can_show_check: bool,
}

/// The text of `RepairCard`.
pub fn repair_card_text(group: &RepairGroup, view: &PrChecksState) -> RepairCardText {
    let mut counts: Vec<(RepairState, usize)> = Vec::new();
    for item in &group.items {
        match counts.iter_mut().find(|(state, _)| *state == item.state) {
            Some((_, count)) => *count += 1,
            None => counts.push((item.state, 1)),
        }
    }
    let lead = PRIORITY
        .into_iter()
        .find(|state| counts.iter().any(|(entry, _)| entry == state))
        .unwrap_or(RepairState::Waiting);
    let total = group.items.len();
    let label = format!("{total} {}", if total == 1 { "check" } else { "checks" });
    let single = (total == 1).then(|| &group.items[0]);
    let clauses = counts
        .iter()
        .map(|(state, count)| {
            let look = repair_look(*state);
            let text = if matches!(state, RepairState::Check(_)) {
                if single.is_some() {
                    look.label.to_string()
                } else {
                    format!("{count} {}", look.summary)
                }
            } else if counts.len() == 1 {
                look.summary.to_string()
            } else {
                format!("{count} {}", look.label.to_lowercase())
            };
            (*state, text)
        })
        .collect();
    let can_show_check = single.is_some_and(|single| {
        single.state == RepairState::Check(GithubPrCheckState::Pass)
            && view.checks.as_ref().is_some_and(|checks| {
                checks
                    .checks
                    .iter()
                    .filter(|check| {
                        check.name == single.check.name && check.workflow == single.check.workflow
                    })
                    .count()
                    == 1
            })
    });
    RepairCardText {
        lead,
        clauses,
        subtitle: match single {
            Some(single) => single.check.name.clone(),
            None => format!("CI repair for {label}"),
        },
        subtitle_title: single
            .map(|single| format!("{}: {}", single.check.workflow, single.check.name)),
        label,
        can_show_check,
    }
}

/// Handlers a repair card calls.
pub struct RepairCardHandlers {
    pub on_toggle: crate::data::Action,
    pub on_show_check: Option<crate::data::Action>,
    pub on_open_session: Option<crate::data::Action>,
}

/// `RepairCard`: one chat's repair, collapsed to its counts, expanding to
/// the included checks and the latest commit.
pub fn repair_card(
    index: usize,
    group: &RepairGroup,
    view: &PrChecksState,
    expanded: bool,
    handlers: RepairCardHandlers,
    cx: &App,
) -> AnyElement {
    let theme = Theme::of(cx);
    let text = repair_card_text(group, view);
    let RepairCardHandlers {
        on_toggle,
        on_show_check,
        on_open_session,
    } = handlers;
    let can_show = text.can_show_check && on_show_check.is_some();
    let toggle = div()
        .id(("repair-toggle", index))
        .flex()
        .flex_1()
        .min_w_0()
        .items_center()
        .gap(u(10.))
        .rounded(u(theme.radius.sm))
        .on_click(move |_, window, cx| on_toggle(window, cx))
        .child(status_icon(
            ElementId::NamedInteger("repair-lead".into(), index as u64),
            text.lead,
            theme,
        ))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_x(u(12.))
                        .gap_y(u(2.))
                        .text_px(theme.text.label)
                        .medium()
                        .children(text.clauses.iter().map(|(state, clause)| {
                            div()
                                .text_color(repair_ink(*state, theme))
                                .child(clause.clone())
                        })),
                )
                .child({
                    let subtitle = div()
                        .id(("repair-subtitle", index))
                        .mt(u(2.))
                        .truncate()
                        .text_px(theme.text.caption)
                        .text_color(theme.content(0.65))
                        .child(text.subtitle.clone());
                    match text.subtitle_title.clone() {
                        Some(title) => subtitle
                            .tooltip(monocode_ui::widgets::tooltip(title))
                            .into_any_element(),
                        None => subtitle.into_any_element(),
                    }
                }),
        )
        .child(
            icon(if expanded {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            })
            .size(u(12.))
            .text_color(theme.content(0.40)),
        );
    let selection_hover = theme.colors.selection;
    let content_hover = theme.colors.content;
    let mut header = div()
        .flex()
        .min_w_0()
        .flex_wrap()
        .items_center()
        .gap_x(u(12.))
        .gap_y(u(4.))
        .px(u(12.))
        .py(u(10.))
        .child(toggle);
    if let (true, Some(show)) = (can_show, on_show_check) {
        header = header.child(
            div()
                .id(("repair-show-check", index))
                .flex_none()
                .rounded(u(theme.radius.md))
                .px(u(8.))
                .py(u(6.))
                .text_px(theme.text.caption)
                .text_color(theme.content(0.70))
                .hover(move |s| s.bg(selection_hover).text_color(content_hover))
                .on_click(move |_, window, cx| show(window, cx))
                .child("Show check"),
        );
    }
    if let Some(open) = on_open_session {
        let ink = theme.content(0.70);
        header = header.child(
            div()
                .id(("repair-open-session", index))
                .group("repair-open")
                .flex()
                .flex_none()
                .items_center()
                .gap(u(6.))
                .rounded(u(theme.radius.md))
                .border_1()
                .border_color(theme.colors.stroke)
                .px(u(10.))
                .py(u(6.))
                .text_px(theme.text.caption)
                .text_color(ink)
                .hover(move |s| s.bg(selection_hover).text_color(content_hover))
                .on_click(move |_, window, cx| open(window, cx))
                .child(
                    icon(IconName::MessageSquare)
                        .size(u(14.))
                        .text_color(ink)
                        .group_hover("repair-open", move |s| s.text_color(content_hover)),
                )
                .child("Open conversation"),
        );
    }
    let head_oid: SharedString = view
        .checks
        .as_ref()
        .map(|checks| checks.head_oid.chars().take(7).collect::<String>())
        .filter(|oid| !oid.is_empty())
        .unwrap_or_else(|| "Unavailable".into())
        .into();
    div()
        .overflow_hidden()
        .rounded(u(theme.radius.lg))
        .border_1()
        .border_color(theme.colors.stroke)
        .bg(theme.content(0.02))
        .child(header)
        .when(expanded, |card| {
            card.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(u(8.))
                    .border_t_1()
                    .border_color(theme.colors.stroke)
                    .px(u(12.))
                    .py(u(10.))
                    .text_px(theme.text.caption)
                    .text_color(theme.content(0.55))
                    .child("Included checks")
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(u(6.))
                            .children(group.items.iter().map(|item| {
                                div()
                                    .max_w_full()
                                    .truncate()
                                    .rounded(u(theme.radius.sm))
                                    .bg(theme.content(0.05))
                                    .px(u(8.))
                                    .py(u(4.))
                                    .text_color(theme.content(0.75))
                                    .child(item.check.name.clone())
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .child("Latest PR commit:\u{a0}")
                            .child(div().font_family(theme.fonts.mono.clone()).child(head_oid))
                            .child(". Results appear in the checks below."),
                    ),
            )
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::CiRepairPhase;

    fn attempt(head_oid: &str, url: &str, phase: CiRepairPhase) -> TrackedCiRepair {
        TrackedCiRepair {
            repo: "acme/web".into(),
            number: 42,
            head_oid: head_oid.into(),
            checks: vec![CiRepairCheck {
                name: "External tests".into(),
                workflow: String::new(),
                url: Some(url.into()),
            }],
            id: format!("repair-{head_oid}"),
            cwd: "/external-ci".into(),
            session_id: "external-chat".into(),
            started_at: date_parse("2030-01-01T10:00:00Z").unwrap(),
            sequence: None,
            phase,
        }
    }

    fn view(checks: Vec<GithubPrCheck>, head_oid: &str) -> PrChecksState {
        PrChecksState {
            checks: Some(GithubPrChecks {
                head_oid: head_oid.into(),
                checks,
            }),
            ..Default::default()
        }
    }

    fn check(state: GithubPrCheckState, url: &str, started_at: Option<&str>) -> GithubPrCheck {
        GithubPrCheck {
            name: "External tests".into(),
            workflow: String::new(),
            state,
            url: Some(url.into()),
            started_at: started_at.map(Into::into),
            completed_at: None,
        }
    }

    #[test]
    fn verifies_a_newer_external_ci_result_even_when_its_dashboard_url_stays_the_same() {
        let url = "https://ci.example/project/web";
        let attempts = [attempt("old", url, CiRepairPhase::Completed)];
        let state = view(
            vec![check(
                GithubPrCheckState::Pass,
                url,
                Some("2030-01-01T10:00:01Z"),
            )],
            "new",
        );
        let groups = check_repairs(&attempts, "/external-ci", "acme/web", Some(42), &state);
        assert_eq!(groups.len(), 1);
        assert_eq!(
            groups[0].items[0].state,
            RepairState::Check(GithubPrCheckState::Pass)
        );
        assert_eq!(repair_look(groups[0].items[0].state).label, "CI passed");
    }

    #[test]
    fn does_not_use_one_newer_result_to_verify_two_different_jobs_with_the_same_name() {
        let url = "https://ci.example/project/web";
        let mut first = attempt("old", url, CiRepairPhase::Completed);
        first.checks.push(CiRepairCheck {
            name: "External tests".into(),
            workflow: String::new(),
            url: Some("https://ci.example/project/web-2".into()),
        });
        let state = view(
            vec![check(
                GithubPrCheckState::Pass,
                url,
                Some("2030-01-01T10:00:01Z"),
            )],
            "new",
        );
        let groups = check_repairs(&[first], "/external-ci", "acme/web", Some(42), &state);
        assert!(
            groups[0]
                .items
                .iter()
                .all(|item| item.state == RepairState::Waiting)
        );
    }

    #[test]
    fn reports_running_and_stale_states_before_results() {
        let url = "https://ci.example/project/web";
        let running = [attempt("old", url, CiRepairPhase::Running)];
        let state = view(vec![], "new");
        let groups = check_repairs(&running, "/external-ci", "acme/web", Some(42), &state);
        assert_eq!(groups[0].items[0].state, RepairState::Repairing);

        let done = [attempt("old", url, CiRepairPhase::Completed)];
        let stale = PrChecksState {
            stale: true,
            error: Some("rate limited".into()),
            ..view(vec![], "new")
        };
        let groups = check_repairs(&done, "/external-ci", "acme/web", Some(42), &stale);
        assert_eq!(groups[0].items[0].state, RepairState::Stale);
        let text = repair_card_text(&groups[0], &stale);
        assert_eq!(text.clauses[0].1, "GitHub results are out of date");
    }

    #[test]
    fn only_matches_the_repaired_job_when_check_names_repeat() {
        let first = check(GithubPrCheckState::Fail, "https://ci.example/jobs/1", None);
        let second = check(GithubPrCheckState::Fail, "https://ci.example/jobs/2", None);
        let mut tracked = attempt("abc", "https://ci.example/jobs/1", CiRepairPhase::Running);
        tracked.cwd = "/duplicate-jobs".into();
        let state = view(vec![first.clone(), second.clone()], "abc");
        let groups = check_repairs(&[tracked], "/duplicate-jobs", "acme/web", Some(42), &state);
        let current = state.checks.as_ref().unwrap();
        assert!(find_check_repair(&groups, &first, current).is_some());
        assert!(find_check_repair(&groups, &second, current).is_none());
    }
}
