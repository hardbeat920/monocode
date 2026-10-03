//! Port of src/features/inbox/ui/InboxPrChecks.tsx: the Checks tab label and
//! the Checks tab body. Rows group by outcome, failures first; GitHub
//! Actions jobs expand to their failed step, annotations, and run steps; a
//! failed refresh keeps the previous rows behind an out-of-date notice; and
//! failed checks can be handed to an agent through [`CheckRepairForm`].

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, Context, ElementId, Entity, Hsla,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, RenderOnce, ScrollHandle,
    SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Task, Transformation,
    Window, canvas, div, percentage, point, prelude::FluentBuilder as _, px,
};
use monocode_ui::widgets::tooltip;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use crate::data::{
    GithubCheckDetails, GithubPrCheck, GithubPrCheckState, InboxServices, PrChecksData,
    PrChecksState,
};
use crate::model::{
    CHECK_STATES, CheckCounts, ChecksOverall, capitalize, check_duration, check_state_label,
    count_check_states, count_checks, describe_check_counts, github_actions_job_id, is_http_url,
    sort_checks,
};
use crate::pr::check_evidence::{EvidenceState, check_evidence};
use crate::pr::repair_form::{CheckRepair, CheckRepairForm, CloseRepairForm};
use crate::pr::repair_progress::{
    RepairCardHandlers, RepairGroup, check_repairs, find_check_repair, repair_card, repair_status,
};
use crate::style::{PopoverAlign, closed_ink, loader, popover_below, spin_icon};

/// An icon, its ink, and whether it spins.
pub struct Mark {
    pub icon: IconName,
    pub ink: Hsla,
    pub spins: bool,
}

/// `overallMark`.
pub fn overall_mark(overall: &ChecksOverall, theme: &Theme) -> Mark {
    let (icon, ink, spins) = match overall {
        ChecksOverall::Loading { .. } => (IconName::LoaderCircle, theme.content(0.45), true),
        ChecksOverall::Error { .. } => (IconName::AlertCircle, closed_ink(), false),
        ChecksOverall::Fail { .. } => (IconName::CircleX, closed_ink(), false),
        ChecksOverall::Pending { .. } => (
            IconName::LoaderCircle,
            monocode_ui::color::with_alpha(theme.colors.warning, 0.7),
            true,
        ),
        ChecksOverall::Pass { .. } => (IconName::CheckCircle, crate::style::open_ink(theme), false),
        ChecksOverall::Neutral { .. } => (IconName::CircleDashed, theme.content(0.45), false),
    };
    Mark { icon, ink, spins }
}

/// `checkMark`.
pub fn check_mark(state: GithubPrCheckState, theme: &Theme) -> Mark {
    let (icon, ink, spins) = match state {
        GithubPrCheckState::Pass => (IconName::CheckCircle, crate::style::open_ink(theme), false),
        GithubPrCheckState::Fail => (IconName::CircleX, closed_ink(), false),
        GithubPrCheckState::Pending => (IconName::LoaderCircle, theme.content(0.55), true),
        GithubPrCheckState::Cancel => (IconName::Minus, theme.content(0.45), false),
        GithubPrCheckState::Unknown => (IconName::CircleHelp, theme.content(0.45), false),
        GithubPrCheckState::Skipping => (IconName::CircleDashed, theme.content(0.40), false),
    };
    Mark { icon, ink, spins }
}

fn mark_icon(id: impl Into<ElementId>, mark: &Mark, size: f32) -> AnyElement {
    if mark.spins {
        spin_icon(id, mark.icon, size, mark.ink)
    } else {
        icon(mark.icon)
            .size(u(size))
            .text_color(mark.ink)
            .into_any_element()
    }
}

type SelectFn = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// `PrChecksTab`: "Checks", an overall mark, and the failure count. The
/// tooltip spells out every count, so color never carries them alone.
#[derive(IntoElement)]
pub struct PrChecksTab {
    overall: ChecksOverall,
    selected: bool,
    on_select: Option<SelectFn>,
}

pub fn pr_checks_tab(overall: ChecksOverall, selected: bool) -> PrChecksTab {
    PrChecksTab {
        overall,
        selected,
        on_select: None,
    }
}

impl PrChecksTab {
    pub fn on_select(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_select = Some(Box::new(handler));
        self
    }

    /// The tab's accessible name, `Checks: <description>`.
    pub fn label(overall: &ChecksOverall) -> String {
        format!("Checks: {}", overall.description())
    }
}

impl RenderOnce for PrChecksTab {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let mark = overall_mark(&self.overall, theme);
        let label = Self::label(&self.overall);
        let ink = if self.selected {
            theme.colors.content
        } else {
            theme.content(0.50)
        };
        let hover = theme.colors.content;
        let mut tab = div()
            .id("pr-checks-tab")
            .relative()
            .flex()
            .h(u(36.))
            .items_center()
            .gap(u(6.))
            .text_px(theme.text.label)
            .leading(theme.leading.none)
            .text_color(ink)
            .tooltip(tooltip(label))
            .child("Checks")
            .child(mark_icon("pr-checks-tab-mark", &mark, 14.));
        if !self.selected {
            tab = tab.hover(move |s| s.text_color(hover));
        }
        if let ChecksOverall::Fail { failed, .. } = &self.overall {
            tab = tab.child(
                div()
                    .tabular()
                    .leading(theme.leading.none)
                    .text_color(mark.ink)
                    .child(failed.to_string()),
            );
        }
        if self.selected {
            tab = tab.child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(px(2.))
                    .bg(theme.colors.content),
            );
        }
        if let Some(handler) = self.on_select {
            tab = tab.on_click(handler);
        }
        tab
    }
}

/// `selectedChecksStillFailed`: every selected check still has a failing
/// row, counting duplicates.
pub fn selected_checks_still_failed(selected: &[GithubPrCheck], current: &[GithubPrCheck]) -> bool {
    let identity = |check: &GithubPrCheck| {
        (
            check.workflow.clone(),
            check.name.clone(),
            check.url.clone(),
        )
    };
    type Identity = (String, String, Option<String>);
    let mut failures: Vec<(Identity, usize)> = Vec::new();
    for check in current {
        if check.state != GithubPrCheckState::Fail {
            continue;
        }
        let key = identity(check);
        match failures.iter_mut().find(|(entry, _)| *entry == key) {
            Some((_, count)) => *count += 1,
            None => failures.push((key, 1)),
        }
    }
    selected.iter().all(|check| {
        let key = identity(check);
        match failures.iter_mut().find(|(entry, _)| *entry == key) {
            Some((_, count)) if *count > 0 => {
                *count -= 1;
                true
            }
            _ => false,
        }
    })
}

/// "Needs attention" or "All checks".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChecksFilter {
    Attention,
    All,
}

/// What a check row shows, for tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckRowInfo {
    pub expanded: bool,
    pub expandable: bool,
    pub linked: bool,
    pub loading: bool,
    pub error: Option<String>,
    pub has_details: bool,
    /// The failure line or "Failed at <step>".
    pub subtitle: Option<String>,
    /// `workflow · status · duration`.
    pub meta: String,
    /// The row's accessible name.
    pub title: String,
    pub steps: Vec<String>,
    pub annotations: usize,
    pub shown_annotations: usize,
    /// The repair chip's label.
    pub repair: Option<String>,
}

/// Where the repair form opened from.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SelectionAnchor {
    FixAll,
    Row(String),
}

struct Selection {
    checks: Vec<GithubPrCheck>,
    anchor: SelectionAnchor,
    scope: String,
}

struct Revealed {
    name: String,
    workflow: String,
    scope: String,
    token: u64,
}

/// One row's own state (`PrCheckRow`'s hooks).
#[derive(Default)]
struct RowState {
    expanded: bool,
    /// `autoExpand && expandable` at the last sync.
    auto: bool,
    details: Option<GithubCheckDetails>,
    error: Option<String>,
    loading: bool,
    details_key: String,
    retry: u64,
    /// The `(details key, checks revision, retry)` the last fetch ran for.
    fetched_for: Option<(String, u64, u64)>,
    generation: u64,
    task: Option<Task<()>>,
    steps_open: bool,
    evidence: EvidenceState,
    /// Bump to scroll the row into view on its next paint.
    reveal: Rc<Cell<bool>>,
    last_reveal_token: u64,
}

/// A sorted row and its identity.
#[derive(Clone)]
struct Row {
    key: String,
    check: GithubPrCheck,
}

/// The Checks tab body.
pub struct PrChecksView {
    services: Rc<dyn InboxServices>,
    data: Rc<dyn PrChecksData>,
    cwd: String,
    repo: String,
    repair: Option<CheckRepair>,
    state: PrChecksState,
    revision: u64,
    filter: ChecksFilter,
    show_others: bool,
    selection: Option<Selection>,
    revealed: Option<Revealed>,
    rows: HashMap<String, RowState>,
    repair_cards: HashMap<String, bool>,
    form: Option<(Entity<CheckRepairForm>, Subscription)>,
    scroll: Option<ScrollHandle>,
    animate: bool,
    repair_groups_snapshot: std::cell::RefCell<Vec<RepairGroup>>,
    _subscriptions: Vec<Subscription>,
}

impl PrChecksView {
    pub fn new(
        services: Rc<dyn InboxServices>,
        data: Rc<dyn PrChecksData>,
        cwd: String,
        repo: String,
        repair: Option<CheckRepair>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut view = Self {
            services,
            data,
            cwd,
            repo,
            repair,
            state: PrChecksState::default(),
            revision: 0,
            filter: ChecksFilter::Attention,
            show_others: false,
            selection: None,
            revealed: None,
            rows: HashMap::new(),
            repair_cards: HashMap::new(),
            form: None,
            scroll: None,
            animate: true,
            repair_groups_snapshot: Default::default(),
            _subscriptions: Vec::new(),
        };
        view.subscribe(cx);
        view.sync(cx);
        view
    }

    fn subscribe(&mut self, cx: &mut Context<Self>) {
        let weak = cx.entity().downgrade();
        let data_sub = self.data.subscribe(
            Box::new(move |cx| {
                if let Some(view) = weak.upgrade() {
                    view.update(cx, |view, cx| view.sync(cx));
                }
            }),
            cx,
        );
        let weak = cx.entity().downgrade();
        let repairs_sub = self.services.subscribe_ci_repairs(
            Box::new(move |cx| {
                if let Some(view) = weak.upgrade() {
                    view.update(cx, |view, cx| view.sync(cx));
                }
            }),
            cx,
        );
        self._subscriptions = vec![data_sub, repairs_sub];
    }

    /// Swap in another pull request's checks data.
    pub fn set_data(&mut self, data: Rc<dyn PrChecksData>, cx: &mut Context<Self>) {
        self.data = data;
        self.subscribe(cx);
        self.sync(cx);
    }

    /// New repair props (sessions, number, handlers).
    pub fn set_repair(&mut self, repair: Option<CheckRepair>, cx: &mut Context<Self>) {
        self.repair = repair;
        self.sync(cx);
    }

    /// The scroller the rows live in, for "Show check".
    pub fn set_scroll_handle(&mut self, scroll: ScrollHandle) {
        self.scroll = Some(scroll);
    }

    /// The Refresh and Retry buttons.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.data.refresh(cx);
    }

    /// Turns popover animations off, for screenshots.
    pub fn set_animate(&mut self, animate: bool, cx: &mut Context<Self>) {
        self.animate = animate;
        if let Some((form, _)) = &self.form {
            form.update(cx, |form, _| form.set_animate(animate));
        }
    }

    pub fn state(&self) -> &PrChecksState {
        &self.state
    }

    pub fn filter(&self) -> ChecksFilter {
        self.active_filter()
    }

    fn counts(&self) -> CheckCounts {
        count_check_states(
            &self
                .sorted_rows()
                .iter()
                .map(|row| row.check.clone())
                .collect::<Vec<_>>(),
        )
    }

    fn attention(&self) -> i64 {
        let counts = self.counts();
        counts.fail + counts.pending + counts.cancel + counts.unknown
    }

    fn active_filter(&self) -> ChecksFilter {
        if self.attention() > 0 {
            self.filter
        } else {
            ChecksFilter::All
        }
    }

    fn reveal_scope(&self) -> String {
        serde_json::json!([
            self.cwd,
            self.repo,
            self.repair.as_ref().map(|repair| repair.number),
            self.state
                .checks
                .as_ref()
                .map(|checks| checks.head_oid.clone()),
        ])
        .to_string()
    }

    fn blocked(&self) -> bool {
        self.state.refreshing
            || self.state.stale
            || self
                .state
                .error
                .as_deref()
                .is_some_and(|error| !error.is_empty())
    }

    fn sorted_rows(&self) -> Vec<Row> {
        let Some(checks) = self.state.checks.as_ref() else {
            return Vec::new();
        };
        let sorted = sort_checks(&checks.checks);
        let number = self.repair.as_ref().map(|repair| repair.number);
        let mut rows = Vec::with_capacity(sorted.len());
        for (index, check) in sorted.iter().enumerate() {
            // Rows of one state that share a workflow, name, and URL keep
            // their order among themselves.
            let state_rows: Vec<&GithubPrCheck> = sorted[..index]
                .iter()
                .filter(|row| row.state == check.state)
                .collect();
            let duplicates = state_rows
                .iter()
                .filter(|row| {
                    row.workflow == check.workflow && row.name == check.name && row.url == check.url
                })
                .count();
            let key = serde_json::json!([
                self.cwd,
                self.repo,
                number,
                checks.head_oid,
                check.workflow,
                check.name,
                check.url,
                duplicates,
            ])
            .to_string();
            rows.push(Row {
                key,
                check: check.clone(),
            });
        }
        rows
    }

    fn repair_groups(&self, cx: &App) -> Vec<RepairGroup> {
        let attempts = self.services.ci_repairs(cx);
        check_repairs(
            &attempts,
            &self.cwd,
            &self.repo,
            self.repair.as_ref().map(|repair| repair.number),
            &self.state,
        )
    }

    /// Reads the data again and runs the row effects.
    pub fn sync(&mut self, cx: &mut Context<Self>) {
        let next = self.data.state(cx);
        if next.checks != self.state.checks || next.generation != self.state.generation {
            self.revision += 1;
        }
        self.state = next;

        // The selection closes when its checks stop failing or the PR moves.
        let scope = self.reveal_scope();
        let selection_valid = match (&self.selection, &self.state.checks) {
            (Some(selection), Some(checks)) => {
                selection.scope == scope
                    && selected_checks_still_failed(&selection.checks, &checks.checks)
            }
            _ => false,
        };
        if self.selection.is_some() && !selection_valid {
            self.selection = None;
            self.form = None;
        }
        if let Some((form, _)) = &self.form {
            let blocked = self.blocked();
            form.update(cx, |form, cx| form.set_blocked(blocked, cx));
        }

        let rows = self.sorted_rows();
        let groups = self.repair_groups(cx);
        *self.repair_groups_snapshot.borrow_mut() = groups.clone();
        let first_fail = rows
            .iter()
            .position(|row| row.check.state == GithubPrCheckState::Fail);
        let head_oid = self
            .state
            .checks
            .as_ref()
            .map(|checks| checks.head_oid.clone())
            .unwrap_or_default();
        let mut keep: HashMap<String, RowState> = HashMap::new();
        let mut loads: Vec<(String, String)> = Vec::new();
        for (index, row) in rows.iter().enumerate() {
            let mut state = self.rows.remove(&row.key).unwrap_or_default();
            let check = &row.check;
            let job_id = github_actions_job_id(check.url.as_deref(), &self.repo);
            let expandable = !self.cwd.is_empty() && job_id.is_some();
            let details_key = serde_json::json!([
                self.cwd,
                self.repo,
                head_oid,
                job_id,
                check.state,
                check.started_at,
                check.completed_at,
            ])
            .to_string();
            if details_key != state.details_key {
                state.details_key = details_key.clone();
                state.details = None;
            }
            let auto = groups.is_empty() && first_fail == Some(index) && expandable;
            if auto && !state.auto {
                state.expanded = true;
            }
            state.auto = auto;
            if let Some(revealed) = &self.revealed
                && revealed.scope == scope
                && revealed.name == check.name
                && revealed.workflow == check.workflow
                && revealed.token != state.last_reveal_token
            {
                state.last_reveal_token = revealed.token;
                if expandable {
                    state.expanded = true;
                }
                state.reveal.set(true);
            }
            if state.expanded && expandable {
                let job_id = job_id.clone().unwrap_or_default();
                let want = (details_key, self.revision, state.retry);
                if state.fetched_for.as_ref() != Some(&want) {
                    state.fetched_for = Some(want);
                    self.fetch_details(&row.key, &mut state, &job_id, cx);
                }
            } else {
                state.fetched_for = None;
                state.task = None;
                state.loading = false;
            }
            if state.expanded
                && let Some(details) = &state.details
            {
                for relative in state
                    .evidence
                    .wanted(&self.cwd, &head_oid, &details.annotations)
                {
                    loads.push((row.key.clone(), relative));
                }
            }
            keep.insert(row.key.clone(), state);
        }
        self.rows = keep;
        for (key, relative) in loads {
            self.load_source(key, relative, &head_oid, cx);
        }
        cx.notify();
    }

    /// Reads one annotated file at the checked commit for a row's evidence.
    fn load_source(
        &mut self,
        key: String,
        relative: String,
        head_oid: &str,
        cx: &mut Context<Self>,
    ) {
        let task = self
            .services
            .commit_file_text(&self.cwd, head_oid, &relative, cx);
        let head = self.evidence_head(head_oid);
        cx.spawn(async move |this, cx| {
            // A failed read shows no preview, like `.catch(() => null)`.
            let text = task.await.ok().flatten();
            let _ = this.update(cx, |this, cx| {
                if let Some(state) = this.rows.get_mut(&key) {
                    state.evidence.finish(&head, relative, text);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn evidence_head(&self, head_oid: &str) -> String {
        format!("{}:{head_oid}", self.cwd)
    }

    fn fetch_details(
        &mut self,
        key: &str,
        state: &mut RowState,
        job_id: &str,
        cx: &mut Context<Self>,
    ) {
        state.generation += 1;
        let generation = state.generation;
        state.loading = true;
        state.error = None;
        let task = self
            .services
            .fetch_check_details(&self.cwd, &self.repo, job_id, cx);
        let key = key.to_string();
        state.task = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                let Some(state) = this.rows.get_mut(&key) else {
                    return;
                };
                if state.generation != generation {
                    return;
                }
                state.loading = false;
                match result {
                    Ok(details) => state.details = Some(details),
                    Err(error) => state.error = Some(error),
                }
                state.task = None;
                // Annotation sources load once the details are in.
                this.sync(cx);
            });
        }));
    }

    /// Expands or collapses a row's details.
    pub fn toggle_row(&mut self, key: &str, cx: &mut Context<Self>) {
        if let Some(state) = self.rows.get_mut(key) {
            state.expanded = !state.expanded;
        }
        self.sync(cx);
    }

    /// Whether the row whose check has this name is expanded.
    pub fn is_expanded(&self, name: &str) -> Option<bool> {
        let rows = self.sorted_rows();
        let row = rows.iter().find(|row| row.check.name == name)?;
        self.rows.get(&row.key).map(|state| state.expanded)
    }

    /// What a row shows, for tests: the first row whose check has this name.
    pub fn row_info(&self, name: &str) -> Option<CheckRowInfo> {
        let rows = self.sorted_rows();
        let row = rows.iter().find(|row| row.check.name == name)?;
        let state = self.rows.get(&row.key)?;
        let check = &row.check;
        let status = check_state_label(check.state);
        let duration = check_duration(check.started_at.as_deref(), check.completed_at.as_deref());
        let workflow = check.workflow.trim().to_string();
        let failed_step = state.details.as_ref().map(|details| {
            details
                .steps
                .iter()
                .filter(|step| step.state == GithubPrCheckState::Fail)
                .map(|step| step.name.clone())
                .collect::<Vec<_>>()
                .join(", ")
        });
        let failed_step = failed_step.filter(|step| !step.is_empty());
        let failure_message = state.details.as_ref().and_then(|details| {
            details
                .annotations
                .iter()
                .find(|annotation| annotation.level == "failure")
                .and_then(|annotation| {
                    annotation
                        .message
                        .lines()
                        .find(|line| !line.trim().is_empty())
                        .map(str::to_string)
                })
        });
        let meta = [
            workflow.clone(),
            status.clone(),
            duration.clone().unwrap_or_default(),
        ]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
        let title = format!(
            "{} · {status}{}{}",
            check.name,
            duration
                .as_ref()
                .map(|duration| format!(", took {duration}"))
                .unwrap_or_default(),
            if workflow.is_empty() {
                String::new()
            } else {
                format!(", {workflow}")
            }
        );
        let groups = self.repair_groups_snapshot.borrow().clone();
        let repair = self
            .state
            .checks
            .as_ref()
            .and_then(|current| find_check_repair(&groups, check, current))
            .map(|item| {
                crate::pr::repair_progress::repair_look(item.state)
                    .label
                    .to_string()
            });
        Some(CheckRowInfo {
            expanded: state.expanded,
            expandable: !self.cwd.is_empty()
                && github_actions_job_id(check.url.as_deref(), &self.repo).is_some(),
            linked: is_http_url(check.url.as_deref()),
            loading: state.loading,
            error: state.error.clone(),
            has_details: state.details.is_some(),
            subtitle: failure_message
                .clone()
                .or_else(|| failed_step.as_ref().map(|step| format!("Failed at {step}"))),
            meta,
            title,
            steps: state
                .details
                .as_ref()
                .map(|details| details.steps.iter().map(|step| step.name.clone()).collect())
                .unwrap_or_default(),
            annotations: state
                .details
                .as_ref()
                .map(|details| details.annotations.len())
                .unwrap_or(0),
            shown_annotations: state.details.as_ref().map_or(0, |details| {
                if state.evidence.show_all {
                    details.annotations.len()
                } else {
                    details.annotations.len().min(5)
                }
            }),
            repair,
        })
    }

    /// "Show N more annotations" on the row whose check has this name.
    pub fn show_all_annotations(&mut self, name: &str, cx: &mut Context<Self>) {
        let rows = self.sorted_rows();
        if let Some(row) = rows.iter().find(|row| row.check.name == name)
            && let Some(state) = self.rows.get_mut(&row.key)
        {
            state.evidence.show_all = true;
        }
        self.sync(cx);
    }

    /// The repair cards: each chat and its text.
    pub fn repair_cards(&self) -> Vec<(String, crate::pr::repair_progress::RepairCardText)> {
        self.repair_groups_snapshot
            .borrow()
            .iter()
            .map(|group| {
                (
                    group.session_id.clone(),
                    crate::pr::repair_progress::repair_card_text(group, &self.state),
                )
            })
            .collect()
    }

    /// Toggles the row whose check has this name, for tests.
    pub fn toggle_named(&mut self, name: &str, cx: &mut Context<Self>) {
        let rows = self.sorted_rows();
        if let Some(row) = rows.iter().find(|row| row.check.name == name) {
            self.toggle_row(&row.key.clone(), cx);
        }
    }

    /// "Retry details" for the row whose check has this name.
    pub fn retry_named(&mut self, name: &str, cx: &mut Context<Self>) {
        let rows = self.sorted_rows();
        if let Some(row) = rows.iter().find(|row| row.check.name == name)
            && let Some(state) = self.rows.get_mut(&row.key)
        {
            state.retry += 1;
        }
        self.sync(cx);
    }

    /// Switches between "Needs attention" and "All checks".
    pub fn set_filter(&mut self, filter: ChecksFilter, cx: &mut Context<Self>) {
        self.filter = filter;
        self.show_others = false;
        self.selection = None;
        self.form = None;
        cx.notify();
    }

    /// The names of the rows on screen, in order.
    pub fn visible_names(&self) -> Vec<String> {
        let rows = self.sorted_rows();
        rows.iter()
            .filter(|row| !self.group_hidden(row.check.state))
            .map(|row| row.check.name.clone())
            .collect()
    }

    fn group_hidden(&self, state: GithubPrCheckState) -> bool {
        self.active_filter() == ChecksFilter::Attention
            && !self.show_others
            && matches!(
                state,
                GithubPrCheckState::Pass | GithubPrCheckState::Skipping
            )
    }

    /// Opens the repair form for every failed check ("Fix all failed").
    pub fn fix_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let checks: Vec<GithubPrCheck> = self
            .sorted_rows()
            .into_iter()
            .filter(|row| row.check.state == GithubPrCheckState::Fail)
            .map(|row| row.check)
            .collect();
        self.open_form(checks, SelectionAnchor::FixAll, window, cx);
    }

    /// Opens the repair form for one failed check, by name.
    pub fn fix_named(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let rows = self.sorted_rows();
        if let Some(row) = rows
            .iter()
            .find(|row| row.check.name == name && row.check.state == GithubPrCheckState::Fail)
        {
            self.open_form(
                vec![row.check.clone()],
                SelectionAnchor::Row(row.key.clone()),
                window,
                cx,
            );
        }
    }

    /// The open repair form.
    pub fn form(&self) -> Option<&Entity<CheckRepairForm>> {
        self.form.as_ref().map(|(form, _)| form)
    }

    fn open_form(
        &mut self,
        checks: Vec<GithubPrCheck>,
        anchor: SelectionAnchor,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(repair) = self.repair.clone() else {
            return;
        };
        let scope = self.reveal_scope();
        let head_oid = self
            .state
            .checks
            .as_ref()
            .map(|checks| checks.head_oid.clone())
            .unwrap_or_default();
        let blocked = self.blocked();
        let services = self.services.clone();
        let cwd = self.cwd.clone();
        let repo = self.repo.clone();
        let animate = self.animate;
        let form_checks = checks.clone();
        let form = cx.new(|cx| {
            let mut form = CheckRepairForm::new(
                services,
                form_checks,
                head_oid,
                cwd,
                repo,
                repair,
                blocked,
                window,
                cx,
            );
            form.set_animate(animate);
            form
        });
        let subscription = cx.subscribe(&form, |this, _, _: &CloseRepairForm, cx| {
            this.selection = None;
            this.form = None;
            cx.notify();
        });
        self.selection = Some(Selection {
            checks,
            anchor,
            scope,
        });
        self.form = Some((form, subscription));
        cx.notify();
    }

    /// "Show check" on a repair card: switch to all checks and reveal the
    /// row.
    pub fn show_check(&mut self, name: &str, workflow: &str, cx: &mut Context<Self>) {
        self.filter = ChecksFilter::All;
        self.selection = None;
        self.form = None;
        let token = self.revealed.as_ref().map(|r| r.token).unwrap_or(0) + 1;
        self.revealed = Some(Revealed {
            name: name.to_string(),
            workflow: workflow.to_string(),
            scope: self.reveal_scope(),
            token,
        });
        self.sync(cx);
    }

    /// Toggles a repair card's details.
    pub fn toggle_repair_card(&mut self, session_id: &str, cx: &mut Context<Self>) {
        let open = self.repair_cards.entry(session_id.to_string()).or_default();
        *open = !*open;
        cx.notify();
    }

    fn render_row(
        &self,
        row: &Row,
        groups: &[RepairGroup],
        wide_status: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let check = &row.check;
        let key = row.key.clone();
        let Some(state) = self.rows.get(&row.key) else {
            return div().into_any_element();
        };
        let id = |suffix: &str| ElementId::Name(format!("{}:{suffix}", row.key).into());
        let job_id = github_actions_job_id(check.url.as_deref(), &self.repo);
        let expandable = !self.cwd.is_empty() && job_id.is_some();
        let expanded = state.expanded;
        let mark = check_mark(check.state, &theme);
        let status = check_state_label(check.state);
        let duration = check_duration(check.started_at.as_deref(), check.completed_at.as_deref());
        let workflow = check.workflow.trim().to_string();
        let failed_step = state.details.as_ref().map(|details| {
            details
                .steps
                .iter()
                .filter(|step| step.state == GithubPrCheckState::Fail)
                .map(|step| step.name.clone())
                .collect::<Vec<_>>()
                .join(", ")
        });
        let failed_step = failed_step.filter(|step| !step.is_empty());
        let failure_message = state.details.as_ref().and_then(|details| {
            details
                .annotations
                .iter()
                .find(|annotation| annotation.level == "failure")
                .and_then(|annotation| {
                    annotation
                        .message
                        .lines()
                        .find(|line| !line.trim().is_empty())
                        .map(str::to_string)
                })
        });
        let subtitle = failure_message
            .clone()
            .or_else(|| failed_step.as_ref().map(|step| format!("Failed at {step}")))
            .unwrap_or_else(|| status.clone());
        let title = format!(
            "{} · {status}{}{}",
            check.name,
            duration
                .as_ref()
                .map(|duration| format!(", took {duration}"))
                .unwrap_or_default(),
            if workflow.is_empty() {
                String::new()
            } else {
                format!(", {workflow}")
            }
        );
        let linked = is_http_url(check.url.as_deref());
        let repair_item = self
            .state
            .checks
            .as_ref()
            .and_then(|current| find_check_repair(groups, check, current));
        let show_subtitle = (repair_item.is_none() || expanded)
            && (failure_message.is_some() || failed_step.is_some());

        let name_line = div()
            .flex()
            .min_w_0()
            .items_baseline()
            .gap(u(10.))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_px(14.)
                    .medium()
                    .leading(theme.leading.snug)
                    .text_color(theme.colors.content)
                    .child(check.name.clone()),
            )
            .when(!workflow.is_empty(), |line| {
                line.child(
                    div()
                        .min_w_0()
                        .flex_shrink(2.)
                        .truncate()
                        .text_px(theme.text.caption)
                        .text_color(theme.content(0.40))
                        .child(workflow.clone()),
                )
            });
        let body = div()
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap(u(10.))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .h(u(28.))
                    .w(u(20.))
                    .items_center()
                    .justify_center()
                    .child(mark_icon(id("mark"), &mark, 16.)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .child(name_line)
                    .when(show_subtitle, |col| {
                        col.child(
                            div()
                                .mt(u(2.))
                                .min_w_0()
                                .truncate()
                                .text_px(theme.text.label)
                                .leading(theme.leading.relaxed)
                                .text_color(theme.content(0.55))
                                .child(subtitle.clone()),
                        )
                    }),
            );
        let main: AnyElement = if expandable {
            let key = key.clone();
            div()
                .id(id("toggle"))
                .flex()
                .flex_1()
                .min_w_0()
                .rounded(u(theme.radius.md))
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_row(&key, cx)))
                .child(body)
                .into_any_element()
        } else if linked {
            let url = check.url.clone().unwrap_or_default();
            let services = self.services.clone();
            div()
                .id(id("open"))
                .flex()
                .flex_1()
                .min_w_0()
                .rounded(u(theme.radius.md))
                .tooltip(tooltip(title.clone()))
                .on_click(move |_, _, cx| services.open_url(&url, cx))
                .child(body)
                .into_any_element()
        } else {
            div()
                .id(id("plain"))
                .flex()
                .flex_1()
                .min_w_0()
                .tooltip(tooltip(title.clone()))
                .child(body)
                .into_any_element()
        };
        let status_cell = div()
            .flex_none()
            .w(u(if wide_status { 96. } else { 56. }))
            .child(match repair_item {
                Some(item) => repair_status(id("repair"), item, cx),
                None => div()
                    .text_px(theme.text.caption)
                    .text_color(mark.ink)
                    .child(status.clone())
                    .into_any_element(),
            });
        let duration_cell = div()
            .flex_none()
            .mr(u(4.))
            .w(u(44.))
            .whitespace_nowrap()
            .text_right()
            .text_px(theme.text.micro)
            .tabular()
            .text_color(theme.content(0.40))
            .child(duration.clone().unwrap_or_default());
        let fix_cell: AnyElement = match (&self.repair, check.state) {
            (Some(_), GithubPrCheckState::Fail) => {
                let disabled = self.blocked();
                let name = check.name.clone();
                let selection_bg = theme.colors.selection;
                let ink = theme.colors.content;
                let mut button = div()
                    .id(id("fix"))
                    .group("check-fix")
                    .flex()
                    .flex_none()
                    .size(u(28.))
                    .items_center()
                    .justify_center()
                    .rounded(u(theme.radius.lg))
                    .bg(theme.content(0.03))
                    .tooltip(tooltip("Fix with AI"))
                    .child(
                        icon(IconName::Sparkles)
                            .size(u(14.))
                            .text_color(theme.content(0.65))
                            .group_hover("check-fix", move |s| s.text_color(ink)),
                    );
                if disabled {
                    button = button.opacity(0.4);
                } else {
                    button =
                        button
                            .hover(move |s| s.bg(selection_bg))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.fix_named(&name, window, cx)
                            }));
                }
                let open_here = matches!(
                    &self.selection,
                    Some(Selection { anchor: SelectionAnchor::Row(anchor), .. }) if *anchor == row.key
                );
                let mut cell = div().relative().flex_none().child(button);
                if open_here && let Some((form, _)) = &self.form {
                    cell = cell.child(popover_below(PopoverAlign::End, 6., form.clone(), cx));
                }
                cell.into_any_element()
            }
            _ => div().flex_none().size(u(28.)).into_any_element(),
        };
        let chevron_cell: AnyElement = if expandable {
            let key = key.clone();
            let hover = theme.content(0.05);
            let ink = theme.colors.content;
            let glyph = icon(IconName::ChevronRight)
                .size(u(12.))
                .text_color(theme.content(0.40))
                .group_hover("check-chevron", move |s| s.text_color(ink));
            let glyph = if expanded {
                glyph.with_transformation(Transformation::rotate(percentage(0.25)))
            } else {
                glyph
            };
            div()
                .id(id("chevron"))
                .group("check-chevron")
                .flex()
                .flex_none()
                .size(u(28.))
                .items_center()
                .justify_center()
                .rounded(u(theme.radius.lg))
                .hover(move |s| s.bg(hover))
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_row(&key, cx)))
                .child(glyph)
                .into_any_element()
        } else {
            div().flex_none().size(u(28.)).into_any_element()
        };
        let link_cell: AnyElement = if linked {
            let url = check.url.clone().unwrap_or_default();
            let services = self.services.clone();
            let hover = theme.content(0.10);
            div()
                .id(id("log"))
                .flex()
                .flex_none()
                .size(u(24.))
                .items_center()
                .justify_center()
                .rounded(u(theme.radius.md))
                .opacity(0.6)
                .group_hover("check-row", |s| s.opacity(1.))
                .hover(move |s| s.bg(hover))
                .tooltip(tooltip("View full log on GitHub"))
                .on_click(move |_, _, cx| services.open_url(&url, cx))
                .child(
                    icon(IconName::ExternalLink)
                        .size(u(12.))
                        .text_color(theme.content(0.45)),
                )
                .into_any_element()
        } else {
            div().flex_none().size(u(24.)).into_any_element()
        };
        let row_hover = theme.content(0.02);
        let header = div()
            .group("check-row")
            .flex()
            .min_w_0()
            .items_center()
            .gap(u(8.))
            .rounded(u(theme.radius.xl))
            .px(u(8.))
            .py(u(8.))
            .hover(move |s| s.bg(row_hover))
            .child(main)
            .child(status_cell)
            .child(duration_cell)
            .child(fix_cell)
            .child(chevron_cell)
            .child(link_cell);

        let reveal = state.reveal.clone();
        let scroll = self.scroll.clone();
        let tracker = canvas(
            move |bounds, window, _| {
                if !reveal.get() {
                    return;
                }
                reveal.set(false);
                let Some(scroll) = scroll.as_ref() else {
                    return;
                };
                let container = scroll.bounds();
                let offset = scroll.offset();
                let delta = bounds.top() - container.top();
                scroll.set_offset(point(offset.x, offset.y - delta));
                window.refresh();
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();

        let mut item = div()
            .id(id("row"))
            .relative()
            .min_w_0()
            .rounded(u(theme.radius.xl))
            .child(tracker)
            .child(header);
        if expanded {
            item = item
                .bg(theme.content(0.02))
                .child(self.render_row_details(row, state, cx));
        }
        item.into_any_element()
    }

    fn render_row_details(
        &self,
        row: &Row,
        state: &RowState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let check = &row.check;
        let id = |suffix: &str| ElementId::Name(format!("{}:{suffix}", row.key).into());
        let mut panel = div()
            .flex()
            .flex_col()
            .min_w_0()
            .gap(u(12.))
            .py(u(12.))
            .pl(u(40.))
            .pr(u(12.))
            .text_px(theme.text.label);
        if state.loading && state.details.is_none() {
            panel = panel.child(
                div()
                    .flex()
                    .items_center()
                    .gap(u(8.))
                    .px(u(8.))
                    .py(u(4.))
                    .text_color(theme.content(0.50))
                    .child(loader(id("steps-loading"), 16., theme.content(0.50)))
                    .child("Loading steps…"),
            );
        }
        if let Some(error) = state.error.clone() {
            let key = row.key.clone();
            let hover = theme.content(0.05);
            panel = panel.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(u(8.))
                    .text_color(theme.content(0.60))
                    .child("Could not load job details.")
                    .child(div().text_px(theme.text.caption).child(error))
                    .child(
                        div()
                            .id(id("retry"))
                            .rounded(u(theme.radius.sm))
                            .px(u(8.))
                            .py(u(4.))
                            .hover(move |s| s.bg(hover))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(state) = this.rows.get_mut(&key) {
                                    state.retry += 1;
                                }
                                this.sync(cx);
                            }))
                            .child("Retry details"),
                    ),
            );
        }
        let Some(details) = state.details.as_ref() else {
            return panel.into_any_element();
        };
        if !details.annotations.is_empty() {
            let key = row.key.clone();
            let head_oid = self
                .state
                .checks
                .as_ref()
                .map(|checks| checks.head_oid.clone())
                .unwrap_or_default();
            panel = panel.child(check_evidence(
                &row.key,
                &details.annotations,
                &state.evidence,
                &self.cwd,
                &self.repo,
                &head_oid,
                self.services.clone(),
                cx.listener(move |this, _, _, cx| {
                    if let Some(state) = this.rows.get_mut(&key) {
                        state.evidence.show_all = true;
                    }
                    this.sync(cx);
                }),
                cx,
            ));
        }
        if details.steps.is_empty() {
            panel = panel.child(
                div()
                    .text_color(theme.content(0.50))
                    .child("No steps reported for this job."),
            );
        } else {
            let key = row.key.clone();
            let open = state.steps_open;
            let ink = theme.colors.content;
            let summary_glyph = icon(IconName::ChevronRight)
                .size(u(12.))
                .text_color(theme.content(0.50))
                .group_hover("check-steps", move |s| s.text_color(ink));
            let summary_glyph = if open {
                summary_glyph.with_transformation(Transformation::rotate(percentage(0.25)))
            } else {
                summary_glyph
            };
            let counts =
                describe_check_counts(&count_checks(details.steps.iter().map(|step| &step.state)))
                    .unwrap_or_default();
            let mut steps = div().flex().flex_col().gap(u(8.)).child(
                div()
                    .id(id("steps"))
                    .group("check-steps")
                    .flex()
                    .items_center()
                    .gap(u(4.))
                    .text_px(theme.text.caption)
                    .text_color(theme.content(0.50))
                    .hover(move |s| s.text_color(ink))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(state) = this.rows.get_mut(&key) {
                            state.steps_open = !state.steps_open;
                        }
                        cx.notify();
                    }))
                    .child(summary_glyph)
                    .child("View run steps")
                    .child(
                        div()
                            .ml_auto()
                            .pl(u(8.))
                            .text_right()
                            .text_px(theme.text.micro)
                            .text_color(theme.content(0.35))
                            .child(counts),
                    ),
            );
            if open {
                let mut list = div().flex().flex_col().gap(u(4.));
                for (index, step) in details.steps.iter().enumerate() {
                    let mark = check_mark(step.state, &theme);
                    let mut line = div()
                        .flex()
                        .items_center()
                        .gap(u(8.))
                        .rounded(u(theme.radius.sm))
                        .px(u(8.))
                        .py(u(4.))
                        .child(mark_icon(
                            ElementId::Name(format!("{}:step:{index}", row.key).into()),
                            &mark,
                            16.,
                        ))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_color(theme.content(0.80))
                                .child(step.name.clone()),
                        )
                        .child(
                            div()
                                .flex_none()
                                .tabular()
                                .text_color(theme.content(0.45))
                                .child(
                                    check_duration(
                                        step.started_at.as_deref(),
                                        step.completed_at.as_deref(),
                                    )
                                    .unwrap_or_default(),
                                ),
                        );
                    if step.state == GithubPrCheckState::Fail {
                        line = line.bg(monocode_ui::color::with_alpha(
                            crate::style::palette::rose_400(),
                            0.05,
                        ));
                    }
                    list = list.child(line);
                }
                steps = steps.child(list);
            }
            panel = panel.child(steps);
        }
        if check.state == GithubPrCheckState::Fail
            && details.annotations.is_empty()
            && details.notice.is_none()
        {
            panel = panel.child(
                div()
                    .mt(u(12.))
                    .text_color(theme.content(0.50))
                    .child("No error annotations reported. View the full log on GitHub."),
            );
        }
        if let Some(notice) = details.notice.clone() {
            panel = panel.child(
                div()
                    .mt(u(12.))
                    .text_color(theme.content(0.50))
                    .child(notice),
            );
        }
        panel.into_any_element()
    }
}

impl Render for PrChecksView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _ = window;
        let theme = Theme::of(cx).clone();
        if self.state.loading {
            return div()
                .flex()
                .justify_center()
                .py(u(40.))
                .child(loader("pr-checks-loading", 16., theme.content(0.40)))
                .into_any_element();
        }
        if self.state.checks.is_none()
            && let Some(error) = self.state.error.clone()
        {
            let data = self.data.clone();
            let hover = theme.content(0.05);
            return div()
                .flex()
                .flex_col()
                .items_start()
                .gap(u(8.))
                .child(
                    div()
                        .text_px(theme.text.body)
                        .text_color(theme.content(0.50))
                        .child(error),
                )
                .child(
                    div()
                        .id("pr-checks-retry")
                        .flex()
                        .h(u(28.))
                        .items_center()
                        .gap(u(6.))
                        .rounded(u(theme.radius.md))
                        .border_1()
                        .border_color(theme.content(0.15))
                        .px(u(12.))
                        .text_px(theme.text.label)
                        .text_color(theme.content(0.80))
                        .hover(move |s| s.bg(hover))
                        .tooltip(tooltip("Retry loading checks"))
                        .on_click(move |_, _, cx| data.refresh(cx))
                        .child(
                            icon(IconName::RefreshCw)
                                .size(u(14.))
                                .text_color(theme.content(0.80)),
                        )
                        .child("Retry"),
                )
                .into_any_element();
        }
        let rows = self.sorted_rows();
        let counts =
            count_check_states(&rows.iter().map(|row| row.check.clone()).collect::<Vec<_>>());
        let attention = counts.fail + counts.pending + counts.cancel + counts.unknown;
        let active_filter = self.active_filter();
        let groups = self.repair_groups(cx);
        let headline = if counts.fail > 0 {
            format!(
                "{} {} a fix",
                counts.fail,
                if counts.fail == 1 {
                    "check needs"
                } else {
                    "checks need"
                }
            )
        } else if counts.pending > 0 {
            format!(
                "{} {} running",
                counts.pending,
                if counts.pending == 1 {
                    "check is"
                } else {
                    "checks are"
                }
            )
        } else if attention > 0 {
            format!(
                "{attention} {} attention",
                if attention == 1 {
                    "check needs"
                } else {
                    "checks need"
                }
            )
        } else if counts.pass > 0 {
            "Checks passed".into()
        } else {
            "No checks ran".into()
        };
        let summary = describe_check_counts(&CheckCounts { fail: 0, ..counts });
        let blocked = self.blocked();

        // Header: headline, summary, Fix all failed, refresh.
        let mut left = div().min_w_0();
        if !rows.is_empty() {
            left = left.child(
                div()
                    .text_px(18.)
                    .medium()
                    .leading(theme.leading.snug)
                    .text_color(theme.colors.content)
                    .child(headline),
            );
            if let Some(summary) = summary {
                left = left.child(
                    div()
                        .mt(u(4.))
                        .text_px(theme.text.label)
                        .text_color(theme.content(0.55))
                        .child(format!("{}.", capitalize(&summary))),
                );
            }
        }
        let mut right = div().flex().flex_none().items_center().gap(u(8.));
        if self.repair.is_some() && counts.fail > 0 {
            let c = theme.colors;
            let mut fix = div()
                .id("pr-checks-fix-all")
                .flex()
                .flex_none()
                .h(u(32.))
                .items_center()
                .gap(u(6.))
                .rounded(u(theme.radius.lg))
                .px(u(10.))
                .text_px(theme.text.label)
                .medium()
                .bg(if blocked {
                    c.primary_disabled
                } else {
                    c.primary
                })
                .text_color(if blocked {
                    c.primary_disabled_foreground
                } else {
                    c.primary_foreground
                })
                .child(
                    icon(IconName::Sparkles)
                        .size(u(14.))
                        .text_color(if blocked {
                            c.primary_disabled_foreground
                        } else {
                            c.primary_foreground
                        }),
                )
                .child("Fix all failed")
                .child(
                    div()
                        .ml(u(4.))
                        .border_l_1()
                        .border_color(monocode_ui::color::with_alpha(c.primary_foreground, 0.2))
                        .pl(u(8.))
                        .text_px(theme.text.micro)
                        .opacity(0.55)
                        .child(counts.fail.to_string()),
                );
            if !blocked {
                let hover = c.primary_hover;
                fix = fix
                    .hover(move |s| s.bg(hover))
                    .on_click(cx.listener(|this, _, window, cx| this.fix_all(window, cx)));
            }
            let mut cell = div().relative().flex_none().child(fix);
            if matches!(
                &self.selection,
                Some(Selection {
                    anchor: SelectionAnchor::FixAll,
                    ..
                })
            ) && let Some((form, _)) = &self.form
            {
                cell = cell.child(popover_below(PopoverAlign::End, 6., form.clone(), cx));
            }
            right = right.child(cell);
        }
        let refresh_glyph = if self.state.refreshing {
            loader("pr-checks-refreshing", 14., theme.content(0.45))
        } else {
            icon(IconName::RefreshCw)
                .size(u(14.))
                .text_color(theme.content(0.45))
                .into_any_element()
        };
        let data = self.data.clone();
        let mut refresh = crate::style::square_button(
            "pr-checks-refresh",
            refresh_glyph,
            false,
            self.state.refreshing,
            cx,
        )
        .tooltip(tooltip("Refresh checks"));
        if !self.state.refreshing {
            refresh = refresh.on_click(move |_, _, cx| data.refresh(cx));
        }
        right = right.child(refresh);

        let mut section = div()
            .id("pr-checks")
            .flex()
            .flex_col()
            .min_w_0()
            .gap(u(8.))
            .child(
                div()
                    .mb(u(12.))
                    .flex()
                    .min_w_0()
                    .flex_wrap()
                    .items_start()
                    .justify_between()
                    .gap(u(12.))
                    .px(u(8.))
                    .child(left)
                    .child(right),
            );

        // Repair progress.
        if let Some(repair) = self.repair.clone()
            && !groups.is_empty()
        {
            let mut cards = div().flex().flex_col().gap(u(8.));
            for (index, group) in groups.iter().enumerate() {
                let expanded = self
                    .repair_cards
                    .get(&group.session_id)
                    .copied()
                    .unwrap_or(false);
                let session = group.session_id.clone();
                let toggle_session = session.clone();
                let entity = cx.entity().downgrade();
                let on_toggle: crate::data::Action = Rc::new(move |_, cx| {
                    if let Some(view) = entity.upgrade() {
                        view.update(cx, |view, cx| view.toggle_repair_card(&toggle_session, cx));
                    }
                });
                let single = (group.items.len() == 1).then(|| group.items[0].check.clone());
                let entity = cx.entity().downgrade();
                let on_show_check: Option<crate::data::Action> = single.map(|check| {
                    Rc::new(move |_: &mut Window, cx: &mut App| {
                        if let Some(view) = entity.upgrade() {
                            view.update(cx, |view, cx| {
                                view.show_check(&check.name, &check.workflow, cx)
                            });
                        }
                    }) as crate::data::Action
                });
                let on_open_session: Option<crate::data::Action> =
                    repair.on_open_session.clone().map(|open| {
                        let session = session.clone();
                        Rc::new(move |window: &mut Window, cx: &mut App| open(&session, window, cx))
                            as crate::data::Action
                    });
                cards = cards.child(repair_card(
                    index,
                    group,
                    &self.state,
                    expanded,
                    RepairCardHandlers {
                        on_toggle,
                        on_show_check,
                        on_open_session,
                    },
                    cx,
                ));
            }
            section = section.child(cards);
        }
        if self.state.stale && self.state.error.is_some() {
            section = section.child(
                div()
                    .px(u(8.))
                    .text_px(theme.text.label)
                    .text_color(theme.content(0.55))
                    .child("Saved results may be out of date."),
            );
        }
        if !rows.is_empty() {
            let mut segmented = div()
                .flex()
                .gap(u(2.))
                .rounded(u(theme.radius.lg))
                .border_1()
                .border_color(theme.colors.stroke)
                .bg(theme.content(0.02))
                .p(u(2.));
            for (filter, label, count) in [
                (ChecksFilter::Attention, "Needs attention", attention),
                (ChecksFilter::All, "All checks", rows.len() as i64),
            ] {
                let pressed = active_filter == filter;
                let disabled = filter == ChecksFilter::Attention && attention == 0;
                let ink = theme.colors.content;
                let mut button = div()
                    .id(SharedString::from(format!("pr-checks-filter-{label}")))
                    .flex()
                    .items_center()
                    .gap(u(8.))
                    .rounded(u(theme.radius.md))
                    .px(u(10.))
                    .py(u(4.))
                    .text_px(theme.text.label)
                    .child(label)
                    .child(
                        div()
                            .tabular()
                            .text_color(theme.content(0.40))
                            .child(count.to_string()),
                    );
                button = if pressed {
                    button
                        .bg(theme.colors.selection)
                        .text_color(ink)
                        .shadow_sm()
                } else {
                    button
                        .text_color(theme.content(0.50))
                        .hover(move |s| s.text_color(ink))
                };
                if disabled {
                    button = button.opacity(0.4);
                } else {
                    button = button
                        .on_click(cx.listener(move |this, _, _, cx| this.set_filter(filter, cx)));
                }
                segmented = segmented.child(button);
            }
            section = section.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(u(12.))
                    .py(u(8.))
                    .child(segmented)
                    .child(
                        div()
                            .text_px(theme.text.micro)
                            .text_color(theme.content(0.40))
                            .child(if counts.fail > 0 {
                                "Failures first"
                            } else {
                                ""
                            }),
                    ),
            );
        }
        if rows.is_empty() {
            section = section.child(
                div()
                    .text_px(theme.text.body)
                    .text_color(theme.content(0.45))
                    .child("No checks reported"),
            );
            return section.into_any_element();
        }
        let wide_status = !groups.is_empty();
        for state in CHECK_STATES {
            let group_rows: Vec<&Row> =
                rows.iter().filter(|row| row.check.state == state).collect();
            if group_rows.is_empty() || self.group_hidden(state) {
                continue;
            }
            let mut list = div().flex().flex_col().gap(u(2.));
            for row in group_rows.iter() {
                list = list.child(self.render_row(row, &groups, wide_status, cx));
            }
            section = section.child(
                div()
                    .child(
                        div()
                            .mb(u(6.))
                            .mt(u(12.))
                            .flex()
                            .items_center()
                            .gap(u(8.))
                            .px(u(8.))
                            .text_px(theme.text.label)
                            .text_color(theme.content(0.55))
                            .child(check_state_label(state))
                            .child(
                                div()
                                    .text_px(theme.text.micro)
                                    .text_color(theme.content(0.35))
                                    .child(group_rows.len().to_string()),
                            ),
                    )
                    .child(list),
            );
        }
        if active_filter == ChecksFilter::Attention && counts.pass + counts.skipping > 0 {
            let open = self.show_others;
            let ink = theme.colors.content;
            let glyph = icon(IconName::ChevronRight)
                .size(u(12.))
                .text_color(theme.content(0.50))
                .group_hover("checks-others", move |s| s.text_color(ink));
            let glyph = if open {
                glyph.with_transformation(Transformation::rotate(percentage(0.25)))
            } else {
                glyph
            };
            section = section.child(
                div()
                    .id("pr-checks-others")
                    .group("checks-others")
                    .mt(u(12.))
                    .flex()
                    .items_center()
                    .gap(u(8.))
                    .border_t_1()
                    .border_color(theme.colors.stroke)
                    .px(u(8.))
                    .pt(u(16.))
                    .text_px(theme.text.caption)
                    .text_color(theme.content(0.50))
                    .hover(move |s| s.text_color(ink))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.show_others = !this.show_others;
                        cx.notify();
                    }))
                    .child(glyph)
                    .child(
                        describe_check_counts(&CheckCounts {
                            pass: counts.pass,
                            skipping: counts.skipping,
                            ..CheckCounts::default()
                        })
                        .unwrap_or_default(),
                    ),
            );
        }
        section.into_any_element()
    }
}
