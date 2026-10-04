//! Port of `InboxDetail`, `CopyBranchNameButton`, `InboxPerson`,
//! `InboxProjectPicker`, and `InboxLabel` from
//! src/features/inbox/ui/InboxView.tsx: one item's pinned header (identity,
//! title, people, branch, review, actions, tabs) over its scrolling body
//! (labels, description, comments, diff, or checks).
//!
//! In the inbox the header stays put and the body scrolls. In the linked
//! panel only the identity row is pinned and everything else scrolls.

use std::rc::Rc;
use std::time::Duration;

use gpui::{
    AnyElement, App, AppContext as _, Context, ElementId, Entity, EventEmitter,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Render, ScrollHandle,
    SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::input::{InputEvent, TextareaState};
use monocode_editor::diff_view::DiffView;
use monocode_layout::paths::{project_name, same_project_path};
use monocode_ui::widgets::{popover_frame, tooltip};
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use crate::data::{
    DataTask, GithubPrAction, InboxDetailData, InboxDetailState, InboxItem, InboxKind,
    InboxProjectOption, InboxProvider, InboxReplyTarget, InboxServices, PrChecksData,
    PrChecksParams, RelatedSession,
};
use crate::model::{
    ChecksOverall, format_relative_time, github_review_decision_label, inbox_item_ref,
    inbox_person_avatar_url, inbox_shows_full_file_diff, item_attention_label, kind_label,
    local_date_time, summarize_pr_checks,
};
use crate::pr::actions::{PrActionsProps, github_pr_actions};
use crate::pr::checks::{PrChecksTab, PrChecksView, pr_checks_tab};
use crate::pr::comments::{
    CommentForm, CommentsProps, MarkdownCache, ReplyMode, comment_form, comment_placeholder,
    inbox_comments,
};
use crate::pr::diff::inbox_pr_diff;
use crate::pr::repair_form::CheckRepair;
use crate::style::{
    ActionKind, PopoverAlign, action_button, centered_loader, closed_ink, inbox_status_mark,
    label_chip, open_ink, person, popover_below, provider_mark, status_ink,
};

/// Where the detail shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DetailMode {
    /// The Inbox page: pinned header, scrolling body.
    #[default]
    Inbox,
    /// The linked work item panel: pinned identity row, one scroller.
    Panel,
}

/// The pull request tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DetailTab {
    #[default]
    Summary,
    Code,
    Checks,
}

/// Hunks or whole files on the Code tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiffMode {
    #[default]
    Hunks,
    Full,
}

/// The detail's props besides the item.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DetailProps {
    pub cwd: String,
    pub projects: Vec<InboxProjectOption>,
    pub related_sessions: Vec<RelatedSession>,
    pub mode: DetailMode,
    pub visible: bool,
    pub revision: u64,
    /// `onDiscuss` is wired: show "Ask".
    pub can_discuss: bool,
    /// `onStart` is wired: show "Send to agent" on issues.
    pub can_start: bool,
    /// `onRepairChecks` is wired: failed checks offer "Fix with AI".
    pub can_repair: bool,
}

/// What the detail asks its owner for.
#[derive(Debug, Clone, PartialEq)]
pub enum InboxDetailEvent {
    /// "Ask" opens the discussion panel.
    Discuss,
    /// A related thread or repair chat.
    OpenSession(String),
    /// A pull request action changed the card.
    ItemChanged(Box<InboxItem>),
}

/// Who and what the header shows, worked out from the item and its
/// details. Kept apart from drawing so the tests can read it.
#[derive(Debug, Clone, PartialEq)]
pub struct DetailHeader {
    pub kind_label: &'static str,
    pub reference: String,
    pub status: String,
    pub attention: String,
    pub source: String,
    pub author: Option<(String, String)>,
    pub assignees: Vec<(String, String)>,
    pub unassigned: bool,
    /// The ISO stamp and its relative form.
    pub created: Option<(String, String)>,
    pub updated: Option<String>,
    pub base_ref: String,
    pub head_ref: String,
    pub review_label: &'static str,
    pub review_decision: String,
    pub external_label: &'static str,
    pub is_pr: bool,
    pub github_pr: bool,
    pub tracker: bool,
    pub choose_start_project: bool,
}

impl DetailHeader {
    pub fn new(item: &InboxItem, state: &InboxDetailState, now: i64) -> Self {
        let linear = item.provider == InboxProvider::Linear;
        let jira = item.provider == InboxProvider::Jira;
        let tracker = linear || jira;
        let gitlab = item.provider == InboxProvider::Gitlab;
        let azure = item.provider == InboxProvider::AzureDevops;
        let is_pr = !tracker && item.kind == InboxKind::Pr;
        let github_pr = item.provider == InboxProvider::Github && item.kind == InboxKind::Pr;
        let external_label = if item.kind == InboxKind::Pr {
            if gitlab {
                "Review on GitLab"
            } else if azure {
                "Review on ADO"
            } else {
                "Review on GitHub"
            }
        } else if linear {
            "Open in Linear"
        } else if jira {
            "Open in Jira"
        } else if gitlab {
            "Open on GitLab"
        } else if azure {
            "Open on ADO"
        } else {
            "Open on GitHub"
        };
        let status = if tracker {
            if item.state.is_empty() {
                crate::model::inbox_item_status(item).to_string()
            } else {
                item.state.clone()
            }
        } else {
            crate::model::inbox_item_status(item).to_string()
        };
        let source = if tracker {
            item.team_name
                .clone()
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| item.repo.clone())
        } else if item.repo.is_empty() {
            if item.project_path.is_empty() {
                String::new()
            } else {
                project_name(&item.project_path)
            }
        } else {
            item.repo.clone()
        };
        let details = state.details.value.as_ref();
        let thread = state.thread.value.as_ref();
        let author_name = details
            .map(|details| details.author.trim().to_string())
            .unwrap_or_default();
        let extra: Vec<(String, String)> = item
            .assignees
            .iter()
            .filter(|person| {
                author_name.is_empty()
                    || person.login.trim().to_lowercase() != author_name.to_lowercase()
            })
            .map(|person| {
                (
                    person.login.clone(),
                    inbox_person_avatar_url(
                        item.provider,
                        &person.login,
                        person.avatar_url.as_deref(),
                    ),
                )
            })
            .collect();
        let show_assignment = !extra.is_empty() || item.assignees.is_empty();
        let pick = |from_details: Option<&Option<String>>, from_thread: Option<&String>| {
            let first = from_details
                .and_then(|value| value.as_deref())
                .map(str::trim)
                .unwrap_or("");
            if !first.is_empty() {
                return first.to_string();
            }
            from_thread
                .map(|value| value.trim().to_string())
                .unwrap_or_default()
        };
        let review_decision = pick(
            details.map(|details| &details.review_decision),
            thread.map(|thread| &thread.review_decision),
        );
        let base_ref = pick(
            details.map(|details| &details.base_ref_name),
            thread.map(|thread| &thread.base_ref_name),
        );
        let head_ref = pick(
            details.map(|details| &details.head_ref_name),
            thread.map(|thread| &thread.head_ref_name),
        );
        let created = item
            .created_at
            .as_ref()
            .map(|iso| (iso.clone(), format_relative_time(iso, now)))
            .filter(|(_, relative)| !relative.is_empty());
        let updated = Some(format_relative_time(&item.updated_at, now)).filter(|t| !t.is_empty());
        Self {
            kind_label: kind_label(item),
            reference: inbox_item_ref(item),
            status,
            attention: item_attention_label(item),
            source,
            author: (!author_name.is_empty()).then(|| {
                let url = inbox_person_avatar_url(
                    item.provider,
                    &author_name,
                    details.and_then(|details| details.author_avatar_url.as_deref()),
                );
                (author_name.clone(), url)
            }),
            unassigned: show_assignment && extra.is_empty(),
            assignees: if show_assignment { extra } else { Vec::new() },
            created,
            updated,
            base_ref,
            head_ref,
            review_label: github_review_decision_label(&review_decision),
            review_decision,
            external_label,
            is_pr,
            github_pr,
            tracker,
            choose_start_project: tracker || ((gitlab || azure) && item.project_path.is_empty()),
        }
    }
}

/// `useGithubPrChecks` parameters for an item.
pub fn pr_checks_params(item: &InboxItem, props: &DetailProps) -> PrChecksParams {
    let is_pr = !item.is_tracker() && item.kind == InboxKind::Pr;
    PrChecksParams {
        cwd: if item.project_path.is_empty() {
            props.cwd.clone()
        } else {
            item.project_path.clone()
        },
        repo: item.repo.clone(),
        number: item.number,
        enabled: item.provider == InboxProvider::Github && item.kind == InboxKind::Pr,
        open: is_pr && item.state.trim().to_lowercase() == "open",
        poll: props.visible,
        revision: props.revision,
    }
}

/// `InboxDetail`.
pub struct InboxDetailView {
    services: Rc<dyn InboxServices>,
    data: Rc<dyn InboxDetailData>,
    item: InboxItem,
    props: DetailProps,
    state: InboxDetailState,
    tab: DetailTab,
    diff_mode: DiffMode,
    diff_view: Option<(crate::data::PrDiff, bool, Entity<DiffView>)>,
    checks_data: Option<Rc<dyn PrChecksData>>,
    checks_overall: Option<ChecksOverall>,
    checks_view: Option<Entity<PrChecksView>>,
    merge_action: GithubPrAction,
    merge_menu_open: bool,
    confirmation: Option<GithubPrAction>,
    start_project: String,
    starting: bool,
    start_error: Option<String>,
    project_picker_open: bool,
    copied: bool,
    comment_field: Entity<TextareaState>,
    draft: String,
    placeholder_replying: bool,
    markdown: MarkdownCache,
    scroll: ScrollHandle,
    animate: bool,
    tasks: Vec<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<InboxDetailEvent> for InboxDetailView {}

impl InboxDetailView {
    pub fn new(
        services: Rc<dyn InboxServices>,
        item: InboxItem,
        props: DetailProps,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let data = services.open_detail(&item, cx);
        let comment_field = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(2, 7)
                .placeholder(comment_placeholder(false))
        });
        let field_events = cx.subscribe(&comment_field, |this, field, event, cx| {
            if matches!(event, InputEvent::Change) {
                this.draft = field.read(cx).value().to_string();
                cx.notify();
            }
        });
        let default_project = props
            .projects
            .iter()
            .find(|project| same_project_path(&project.path, &props.cwd))
            .or(props.projects.first())
            .map(|project| project.path.clone())
            .unwrap_or_else(|| props.cwd.clone());
        let mut view = Self {
            markdown: MarkdownCache::new(services.clone()),
            services,
            data,
            item,
            props,
            state: InboxDetailState::default(),
            tab: DetailTab::Summary,
            diff_mode: DiffMode::Hunks,
            diff_view: None,
            checks_data: None,
            checks_overall: None,
            checks_view: None,
            merge_action: GithubPrAction::Merge,
            merge_menu_open: false,
            confirmation: None,
            start_project: default_project,
            starting: false,
            start_error: None,
            project_picker_open: false,
            copied: false,
            comment_field,
            draft: String::new(),
            placeholder_replying: false,
            scroll: ScrollHandle::new(),
            animate: true,
            tasks: Vec::new(),
            _subscriptions: vec![field_events],
        };
        view.subscribe_data(cx);
        view.open_checks(cx);
        view.sync(cx);
        view
    }

    fn subscribe_data(&mut self, cx: &mut Context<Self>) {
        let weak = cx.entity().downgrade();
        let sub = self.data.subscribe(
            Box::new(move |cx| {
                if let Some(view) = weak.upgrade() {
                    view.update(cx, |view, cx| view.sync(cx));
                }
            }),
            cx,
        );
        self._subscriptions.push(sub);
    }

    fn open_checks(&mut self, cx: &mut Context<Self>) {
        let params = pr_checks_params(&self.item, &self.props);
        if !params.enabled {
            self.checks_data = None;
            self.checks_overall = None;
            return;
        }
        let data = self.services.open_pr_checks(params, cx);
        let weak = cx.entity().downgrade();
        let sub = data.subscribe(
            Box::new(move |cx| {
                if let Some(view) = weak.upgrade() {
                    view.update(cx, |view, cx| view.sync_checks(cx));
                }
            }),
            cx,
        );
        self._subscriptions.push(sub);
        self.checks_data = Some(data);
        self.sync_checks(cx);
    }

    fn sync_checks(&mut self, cx: &mut Context<Self>) {
        self.checks_overall = self.checks_data.as_ref().map(|data| {
            let state = data.state(cx);
            summarize_pr_checks(
                state.loading,
                state.error.as_deref(),
                state.checks.as_ref().map(|checks| checks.checks.as_slice()),
            )
        });
        cx.notify();
    }

    fn sync(&mut self, cx: &mut Context<Self>) {
        self.state = self.data.state(cx);
        if self.tab == DetailTab::Code {
            self.sync_diff(cx);
        }
        cx.notify();
    }

    fn sync_diff(&mut self, cx: &mut Context<Self>) {
        let full_file = self.full_file();
        let Some(diff) = self.state.diff.value.clone() else {
            self.diff_view = None;
            return;
        };
        let same = self
            .diff_view
            .as_ref()
            .is_some_and(|(current, mode, _)| *current == diff && *mode == full_file);
        if !same {
            let view = inbox_pr_diff(&diff, full_file, cx);
            self.diff_view = Some((diff, full_file, view));
        }
    }

    fn full_file(&self) -> bool {
        inbox_shows_full_file_diff(&self.item) && self.diff_mode == DiffMode::Full
    }

    pub fn item(&self) -> &InboxItem {
        &self.item
    }

    pub fn tab(&self) -> DetailTab {
        self.tab
    }

    pub fn header(&self) -> DetailHeader {
        DetailHeader::new(&self.item, &self.state, self.services.now_ms())
    }

    pub fn checks_overall(&self) -> Option<&ChecksOverall> {
        self.checks_overall.as_ref()
    }

    pub fn checks_view(&self) -> Option<&Entity<PrChecksView>> {
        self.checks_view.as_ref()
    }

    pub fn start_project(&self) -> &str {
        &self.start_project
    }

    /// Turns popover animations off, for screenshots.
    pub fn set_animate(&mut self, animate: bool, cx: &mut Context<Self>) {
        self.animate = animate;
        if let Some(view) = &self.checks_view {
            view.update(cx, |view, cx| view.set_animate(animate, cx));
        }
    }

    /// The same item with fresh fields.
    pub fn set_item(&mut self, item: InboxItem, cx: &mut Context<Self>) {
        if item == self.item {
            return;
        }
        let params_before = pr_checks_params(&self.item, &self.props);
        self.item = item.clone();
        self.data.set_item(item, cx);
        self.update_checks_params(params_before, cx);
        cx.notify();
    }

    /// New props from the owner (projects, related threads, revision,
    /// visibility).
    pub fn set_props(&mut self, props: DetailProps, cx: &mut Context<Self>) {
        if props == self.props {
            return;
        }
        let params_before = pr_checks_params(&self.item, &self.props);
        let revision = props.revision;
        self.props = props;
        self.data.set_revision(revision, cx);
        self.update_checks_params(params_before, cx);
        cx.notify();
    }

    fn update_checks_params(&mut self, before: PrChecksParams, cx: &mut Context<Self>) {
        let after = pr_checks_params(&self.item, &self.props);
        if after == before {
            return;
        }
        match (&self.checks_data, after.enabled) {
            (Some(data), true) => data.set_params(after, cx),
            _ => self.open_checks(cx),
        }
    }

    /// Switches the pull request tab.
    pub fn set_tab(&mut self, tab: DetailTab, window: &mut Window, cx: &mut Context<Self>) {
        let _ = window;
        self.tab = tab;
        match tab {
            DetailTab::Code => {
                self.data.show_diff(self.full_file(), cx);
                self.sync_diff(cx);
                self.checks_view = None;
            }
            DetailTab::Checks => {
                if self.checks_view.is_none()
                    && let Some(data) = self.checks_data.clone()
                {
                    let repair = self.check_repair(cx);
                    let services = self.services.clone();
                    let cwd = pr_checks_params(&self.item, &self.props).cwd;
                    let repo = self.item.repo.clone();
                    let scroll = self.scroll.clone();
                    let animate = self.animate;
                    self.checks_view = Some(cx.new(|cx| {
                        let mut view = PrChecksView::new(services, data, cwd, repo, repair, cx);
                        view.set_scroll_handle(scroll);
                        view.set_animate(animate, cx);
                        view
                    }));
                }
            }
            DetailTab::Summary => self.checks_view = None,
        }
        cx.notify();
    }

    /// Switches between hunks and whole files on the Code tab.
    pub fn set_diff_mode(&mut self, mode: DiffMode, cx: &mut Context<Self>) {
        self.diff_mode = mode;
        if self.tab == DetailTab::Code {
            self.data.show_diff(self.full_file(), cx);
            self.sync_diff(cx);
        }
        cx.notify();
    }

    fn check_repair(&self, cx: &mut Context<Self>) -> Option<CheckRepair> {
        if !self.props.can_repair
            || self.item.provider != InboxProvider::Github
            || self.item.project_path.is_empty()
        {
            return None;
        }
        let services = self.services.clone();
        let item = self.item.clone();
        let weak = cx.entity().downgrade();
        Some(CheckRepair {
            number: self.item.number,
            sessions: self.services.repair_sessions(&self.item, cx),
            on_start: Rc::new(move |start, session_id, cx| {
                services.repair_checks(&item, start, session_id, cx)
            }),
            on_open_session: Some(Rc::new(move |id, _, cx| {
                if let Some(view) = weak.upgrade() {
                    let id = id.to_string();
                    view.update(cx, |_, cx| cx.emit(InboxDetailEvent::OpenSession(id)));
                }
            })),
        })
    }

    /// "Send to agent".
    pub fn start(&mut self, cx: &mut Context<Self>) {
        if self.starting {
            return;
        }
        let header = self.header();
        self.starting = true;
        self.start_error = None;
        let mut next = self.item.clone();
        if header.choose_start_project {
            next.project_path = self.start_project.clone();
        }
        let body = if header.tracker {
            self.state
                .details
                .value
                .as_ref()
                .map(|details| details.body.clone())
        } else {
            None
        };
        let task: DataTask<()> = self.services.start_item(next, body, cx);
        self.tasks.push(cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.starting = false;
                if let Err(error) = result {
                    this.start_error = Some(error);
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Opens the confirmation for a pull request action.
    pub fn ask_to_run(&mut self, action: GithubPrAction, cx: &mut Context<Self>) {
        self.merge_menu_open = false;
        self.confirmation = Some(action);
        cx.notify();
    }

    /// "Cancel" or a click outside the confirmation.
    pub fn dismiss_confirmation(&mut self, cx: &mut Context<Self>) {
        if self.state.action_busy {
            return;
        }
        self.confirmation = None;
        cx.notify();
    }

    /// The confirmation's main button.
    pub fn run_action(&mut self, cx: &mut Context<Self>) {
        let Some(action) = self.confirmation else {
            return;
        };
        if self.state.action_busy {
            return;
        }
        let task = self.data.run_pr_action(action, cx);
        self.tasks.push(cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if let Ok(next) = result {
                    this.confirmation = None;
                    this.item = next.clone();
                    cx.emit(InboxDetailEvent::ItemChanged(Box::new(next)));
                }
                cx.notify();
            });
        }));
    }

    /// Posts the draft as a comment or reply.
    pub fn submit_comment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let body = self.draft.trim().to_string();
        if body.is_empty() || self.state.posting {
            return;
        }
        let task = self.data.post_comment(body, cx);
        let field = self.comment_field.clone();
        let window_handle = window.window_handle();
        self.tasks.push(cx.spawn(async move |this, cx| {
            if task.await.is_ok() {
                let _ = window_handle.update(cx, |_, window, cx| {
                    field.update(cx, |field, cx| field.set_value("", window, cx));
                });
                let _ = this.update(cx, |this, cx| {
                    this.draft.clear();
                    cx.notify();
                });
            }
        }));
    }

    /// Whether the header lists related threads (the inbox only).
    pub fn related_visible(&self) -> bool {
        self.props.mode == DetailMode::Inbox && !self.props.related_sessions.is_empty()
    }

    /// The copy button beside the branch names. False when there is no
    /// branch to copy.
    pub fn copy_head_branch(&mut self, cx: &mut Context<Self>) -> bool {
        let header = self.header();
        if header.base_ref.is_empty() || header.head_ref.is_empty() {
            return false;
        }
        self.copy_branch(header.head_ref, cx);
        true
    }

    pub fn copied(&self) -> bool {
        self.copied
    }

    pub fn draft(&self) -> &str {
        &self.draft
    }

    /// Types into the comment field.
    pub fn set_draft(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.comment_field.update(cx, |field, cx| {
            field.set_value(text.to_string(), window, cx)
        });
        self.draft = text.to_string();
        cx.notify();
    }

    fn copy_branch(&mut self, branch: String, cx: &mut Context<Self>) {
        if !self.services.copy_text(&branch, cx) {
            return;
        }
        self.services.play_copy_cue(cx);
        self.copied = true;
        cx.notify();
        self.tasks.push(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(2000))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.copied = false;
                cx.notify();
            });
        }));
    }

    fn render_identity_row(&self, header: &DetailHeader, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let panel = self.props.mode == DetailMode::Panel;
        let mark = inbox_status_mark(&self.item);
        let mark_ink = status_ink(mark.tone, &theme);
        let mut row = div()
            .flex()
            .min_w_0()
            .items_center()
            .gap(u(8.))
            .text_px(theme.text.label)
            .text_color(theme.content(0.50))
            .child(provider_mark(self.item.provider, 14., theme.content(0.50)))
            .child(div().flex_none().child(header.kind_label))
            .child(div().flex_none().tabular().child(header.reference.clone()))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(u(4.))
                    .text_color(mark_ink)
                    .child(icon(mark.icon).size(u(14.)).text_color(mark_ink))
                    .child(header.status.clone()),
            );
        if !header.attention.is_empty() {
            row = row.child(
                div()
                    .flex_none()
                    .text_color(theme.colors.accent)
                    .child(header.attention.clone()),
            );
        }
        if !header.source.is_empty() {
            row = row.child(div().min_w_0().truncate().child(header.source.clone()));
        }
        if panel {
            let url = self.item.url.clone();
            let services = self.services.clone();
            let disabled = url.is_empty();
            let title = if disabled {
                "No link available"
            } else {
                header.external_label
            };
            let mut button = action_button(
                "detail-external",
                ActionKind::PanelHeader,
                Some(IconName::ExternalLink),
                header.external_label,
                disabled,
                cx,
            )
            .ml_auto()
            .tooltip(tooltip(title));
            if !disabled {
                button = button.on_click(move |_, _, cx| services.open_url(&url, cx));
            }
            row = row
                .h(u(36.))
                .flex_none()
                .border_b_1()
                .border_color(theme.colors.stroke)
                .px(u(16.))
                .pr(u(34.))
                .child(button);
        }
        row.into_any_element()
    }

    fn render_meta_row(&self, header: &DetailHeader, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let dot = || div().flex_none().child("·");
        let mut row = div()
            .flex()
            .min_w_0()
            .items_center()
            .gap(u(8.))
            .overflow_hidden()
            .whitespace_nowrap()
            .text_px(theme.text.label)
            .text_color(theme.content(0.50));
        if let Some((name, url)) = &header.author {
            row = row.child(person(name, url, 16., cx));
        }
        if !header.assignees.is_empty() || header.unassigned {
            if header.author.is_some() {
                row = row.child(dot());
            }
            if header.unassigned {
                row = row.child(div().flex_none().child("Unassigned"));
            } else {
                row = row.child(
                    div()
                        .flex()
                        .min_w_0()
                        .items_center()
                        .gap(u(8.))
                        .overflow_hidden()
                        .children(
                            header
                                .assignees
                                .iter()
                                .map(|(login, url)| person(login, url, 16., cx)),
                        ),
                );
            }
        }
        if let Some((iso, relative)) = &header.created {
            row = row.child(dot()).child(
                div()
                    .id("detail-created")
                    .flex_none()
                    .tooltip(tooltip(local_date_time(iso)))
                    .child(format!("Created {relative}")),
            );
        }
        if let Some(updated) = &header.updated {
            row = row
                .child(dot())
                .child(div().flex_none().child(format!("Updated {updated}")));
        }
        if !header.base_ref.is_empty() && !header.head_ref.is_empty() {
            let head = header.head_ref.clone();
            let copied = self.copied;
            let hover = theme.content(0.08);
            let hover_ink = theme.content(0.70);
            row = row.child(dot()).child(
                div()
                    .flex()
                    .min_w_0()
                    .items_center()
                    .gap(u(4.))
                    .child(
                        icon(IconName::GitCompare)
                            .size(u(12.))
                            .text_color(theme.content(0.50)),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .child(format!("{} ← {}", header.base_ref, header.head_ref)),
                    )
                    .child(
                        div()
                            .id("detail-copy-branch")
                            .group("copy-branch")
                            .flex_none()
                            .rounded(u(theme.radius.sm))
                            .p(u(2.))
                            .hover(move |s| s.bg(hover))
                            .tooltip(tooltip(if copied { "Copied" } else { "Copy branch name" }))
                            .on_click(
                                cx.listener(move |this, _, _, cx| {
                                    this.copy_branch(head.clone(), cx)
                                }),
                            )
                            .child(
                                icon(if copied {
                                    IconName::Check
                                } else {
                                    IconName::Copy
                                })
                                .size(u(12.))
                                .text_color(theme.content(0.40))
                                .group_hover("copy-branch", move |s| s.text_color(hover_ink)),
                            ),
                    ),
            );
        }
        if !header.review_label.is_empty() {
            let decision = header.review_decision.to_uppercase();
            let ink = if decision == "APPROVED" {
                open_ink(&theme)
            } else if decision == "CHANGES_REQUESTED" {
                closed_ink()
            } else {
                theme.content(0.50)
            };
            row = row
                .child(dot())
                .child(div().flex_none().text_color(ink).child(header.review_label));
        }
        row.into_any_element()
    }

    fn render_related(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.props.mode == DetailMode::Panel || self.props.related_sessions.is_empty() {
            return None;
        }
        let theme = Theme::of(cx).clone();
        let count = self.props.related_sessions.len();
        let mut row = div()
            .flex()
            .min_w_0()
            .items_center()
            .gap(u(6.))
            .overflow_hidden()
            .child(
                div()
                    .mr(u(2.))
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(u(4.))
                    .text_px(theme.text.caption)
                    .text_color(theme.content(0.45))
                    .child(
                        icon(IconName::MessageMultiple)
                            .size(u(14.))
                            .text_color(theme.content(0.45)),
                    )
                    .child(if count == 1 {
                        "Related thread"
                    } else {
                        "Related threads"
                    }),
            );
        for (index, session) in self.props.related_sessions.iter().enumerate() {
            let id = session.id.clone();
            let hover = theme.content(0.10);
            let ink = theme.colors.content;
            row = row.child(
                div()
                    .id(("related-thread", index))
                    .flex()
                    .min_w_0()
                    .max_w(u(256.))
                    .items_center()
                    .gap(u(4.))
                    .rounded(u(theme.radius.md))
                    .bg(theme.content(0.05))
                    .px(u(8.))
                    .py(u(4.))
                    .text_px(theme.text.caption)
                    .text_color(theme.content(0.70))
                    .hover(move |s| s.bg(hover).text_color(ink))
                    .tooltip(tooltip(format!("Open thread: {}", session.title)))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.emit(InboxDetailEvent::OpenSession(id.clone()))
                    }))
                    .child(div().truncate().child(session.title.clone()))
                    .when(session.archived, |chip| {
                        chip.child(
                            div()
                                .flex_none()
                                .text_color(theme.content(0.40))
                                .child("Archived"),
                        )
                    }),
            );
        }
        Some(row.into_any_element())
    }

    fn render_project_picker(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let projects = &self.props.projects;
        let selected = projects
            .iter()
            .find(|project| same_project_path(&project.path, &self.start_project))
            .or(projects.first());
        let disabled = projects.is_empty();
        let hover = theme.content(0.10);
        let ink = theme.colors.content;
        let mut button = div()
            .id("detail-project-picker")
            .flex()
            .h(u(28.))
            .max_w(u(192.))
            .items_center()
            .gap(u(6.))
            .rounded(u(theme.radius.md))
            .border_1()
            .border_color(theme.content(0.10))
            .bg(theme.content(0.05))
            .px(u(8.))
            .text_px(theme.text.label)
            .text_color(theme.content(0.80))
            .when_some(selected, |button, project| {
                button.child(self.services.project_mark(&project.mark, 12., cx))
            })
            .child(
                div().min_w_0().truncate().child(
                    selected
                        .map(|project| project.name.clone())
                        .unwrap_or_else(|| "Choose project".into()),
                ),
            )
            .child(
                icon(IconName::ChevronDown)
                    .size(u(12.))
                    .text_color(theme.content(0.45)),
            );
        if disabled {
            button = button.opacity(0.4);
        } else {
            button = button
                .hover(move |s| s.bg(hover).text_color(ink))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.project_picker_open = !this.project_picker_open;
                    cx.notify();
                }));
        }
        let mut cell = div().relative().flex_none().child(button);
        if self.project_picker_open {
            let mut list = div()
                .id("detail-project-list")
                .flex()
                .flex_col()
                .max_h(u(256.))
                .overflow_y_scroll()
                .p(u(4.))
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    this.project_picker_open = false;
                    cx.notify();
                }));
            for (index, project) in projects.iter().enumerate() {
                let active = selected
                    .is_some_and(|selected| same_project_path(&project.path, &selected.path));
                let path = project.path.clone();
                let hover = theme.content(0.05);
                let mut row = div()
                    .id(("detail-project", index))
                    .flex()
                    .h(u(28.))
                    .w_full()
                    .items_center()
                    .gap(u(6.))
                    .rounded(u(theme.radius.md))
                    .px(u(8.))
                    .text_px(theme.text.label)
                    .child(self.services.project_mark(&project.mark, 12., cx))
                    .child(div().min_w_0().truncate().child(project.name.clone()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.start_project = path.clone();
                        this.project_picker_open = false;
                        cx.notify();
                    }));
                row = if active {
                    row.bg(theme.colors.selection).text_color(ink)
                } else {
                    row.text_color(theme.content(0.80))
                        .hover(move |s| s.bg(hover).text_color(ink))
                };
                list = list.child(row);
            }
            cell = cell.child(popover_below(
                PopoverAlign::Start,
                4.,
                popover_frame("detail-project-menu")
                    .width(256.)
                    .animate(self.animate)
                    .child(list),
                cx,
            ));
        }
        cell.into_any_element()
    }

    fn render_actions(&self, header: &DetailHeader, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let mut row = div().flex().flex_wrap().items_center().gap(u(8.)).pt(u(2.));
        if self.props.can_start && self.item.kind != InboxKind::Pr {
            let disabled = self.starting
                || (header.choose_start_project
                    && (self.props.projects.is_empty()
                        || self.start_project.is_empty()
                        || self.state.details.loading
                        || self.state.details.error.is_some()));
            let mut send = action_button(
                "detail-send",
                ActionKind::Filled,
                None,
                if self.starting {
                    "Sending..."
                } else {
                    "Send to agent"
                },
                disabled,
                cx,
            );
            if !disabled {
                send = send.on_click(cx.listener(|this, _, _, cx| this.start(cx)));
            }
            row = row.child(send);
            if header.choose_start_project {
                row = row.child(self.render_project_picker(cx));
            }
        }
        if header.github_pr {
            let weak = cx.entity().downgrade();
            let on_ask = {
                let weak = weak.clone();
                Rc::new(move |action, _: &mut Window, cx: &mut App| {
                    if let Some(view) = weak.upgrade() {
                        view.update(cx, |view, cx| view.ask_to_run(action, cx));
                    }
                })
            };
            let on_toggle = {
                let weak = weak.clone();
                Rc::new(move |_: &mut Window, cx: &mut App| {
                    if let Some(view) = weak.upgrade() {
                        view.update(cx, |view, cx| {
                            view.merge_menu_open = !view.merge_menu_open;
                            cx.notify();
                        });
                    }
                })
            };
            let on_pick = {
                let weak = weak.clone();
                Rc::new(move |action, _: &mut Window, cx: &mut App| {
                    if let Some(view) = weak.upgrade() {
                        view.update(cx, |view, cx| {
                            view.merge_action = action;
                            view.merge_menu_open = false;
                            cx.notify();
                        });
                    }
                })
            };
            let on_dismiss = {
                let weak = weak.clone();
                Rc::new(move |_: &mut Window, cx: &mut App| {
                    if let Some(view) = weak.upgrade() {
                        view.update(cx, |view, cx| view.dismiss_confirmation(cx));
                    }
                })
            };
            let on_confirm = Rc::new(move |_: &mut Window, cx: &mut App| {
                if let Some(view) = weak.upgrade() {
                    view.update(cx, |view, cx| view.run_action(cx));
                }
            });
            row = row.children(github_pr_actions(
                PrActionsProps {
                    item: self.item.clone(),
                    base_ref: header.base_ref.clone(),
                    head_ref: header.head_ref.clone(),
                    merge_action: self.merge_action,
                    merge_menu_open: self.merge_menu_open,
                    confirmation: self.confirmation,
                    busy: self.state.action_busy,
                    error: self.confirmation.and(self.state.action_error.clone()),
                    notice: self.state.action_notice.clone(),
                    animate: self.animate,
                    on_ask,
                    on_toggle_merge_menu: on_toggle,
                    on_pick_merge: on_pick,
                    on_dismiss,
                    on_confirm,
                },
                cx,
            ));
        }
        if self.props.can_discuss {
            row = row.child(
                action_button(
                    "detail-ask",
                    ActionKind::Outline,
                    Some(IconName::MessageSquare),
                    "Ask",
                    false,
                    cx,
                )
                .on_click(cx.listener(|_, _, _, cx| cx.emit(InboxDetailEvent::Discuss))),
            );
        }
        if self.props.mode == DetailMode::Inbox {
            let url = self.item.url.clone();
            let disabled = url.is_empty();
            let services = self.services.clone();
            let mut button = action_button(
                "detail-open",
                ActionKind::Ghost,
                Some(IconName::ExternalLink),
                header.external_label,
                disabled,
                cx,
            )
            .tooltip(tooltip(if disabled {
                "No link available"
            } else {
                header.external_label
            }));
            if !disabled {
                button = button.on_click(move |_, _, cx| services.open_url(&url, cx));
            }
            row = row.child(button);
        }
        let mut column = div().flex().flex_col().child(row);
        if let Some(error) = self.start_error.clone() {
            column = column.child(
                div()
                    .mt(u(8.))
                    .text_px(theme.text.label)
                    .text_color(monocode_ui::color::with_alpha(theme.colors.danger, 0.9))
                    .child(error),
            );
        }
        column.into_any_element()
    }

    fn render_tabs(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let tab = |id: &'static str,
                   label: &'static str,
                   which: DetailTab,
                   cx: &mut Context<Self>| {
            let selected = self.tab == which;
            let hover = theme.colors.content;
            let mut el = div()
                .id(id)
                .relative()
                .flex()
                .h(u(36.))
                .items_center()
                .text_px(theme.text.label)
                .leading(theme.leading.none)
                .text_color(if selected {
                    theme.colors.content
                } else {
                    theme.content(0.50)
                })
                .on_click(cx.listener(move |this, _, window, cx| this.set_tab(which, window, cx)))
                .child(label);
            if selected {
                el = el.child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .h(px(2.))
                        .bg(theme.colors.content),
                );
            } else {
                el = el.hover(move |s| s.text_color(hover));
            }
            el
        };
        let mut tabs = div()
            .flex()
            .items_stretch()
            .gap(u(16.))
            .child(tab("detail-tab-summary", "Summary", DetailTab::Summary, cx))
            .child(tab("detail-tab-code", "Code", DetailTab::Code, cx));
        if let Some(overall) = self.checks_overall.clone() {
            tabs = tabs.child(
                pr_checks_tab(overall, self.tab == DetailTab::Checks).on_select(
                    cx.listener(|this, _, window, cx| this.set_tab(DetailTab::Checks, window, cx)),
                ),
            );
        }
        let mut row = div()
            .flex()
            .h(u(36.))
            .items_stretch()
            .gap(u(16.))
            .child(tabs);
        if self.tab == DetailTab::Code && inbox_shows_full_file_diff(&self.item) {
            let mut group = div()
                .ml_auto()
                .flex()
                .items_center()
                .self_center()
                .rounded(u(theme.radius.md))
                .border_1()
                .border_color(theme.content(0.10))
                .bg(theme.content(0.03))
                .p(u(2.));
            for (mode, label) in [(DiffMode::Hunks, "Hunks"), (DiffMode::Full, "Full file")] {
                let pressed = self.diff_mode == mode;
                let hover = theme.content(0.70);
                let mut button = div()
                    .id(SharedString::from(format!("diff-mode-{label}")))
                    .rounded(u(theme.radius.sm))
                    .px(u(10.))
                    .py(u(4.))
                    .text_px(theme.text.caption)
                    .leading(theme.leading.none)
                    .on_click(cx.listener(move |this, _, _, cx| this.set_diff_mode(mode, cx)))
                    .child(label);
                button = if pressed {
                    button
                        .bg(theme.colors.selection)
                        .text_color(theme.colors.content)
                } else {
                    button
                        .text_color(theme.content(0.45))
                        .hover(move |s| s.text_color(hover))
                };
                group = group.child(button);
            }
            row = row.child(group);
        }
        row.into_any_element()
    }

    fn render_header(&self, header: &DetailHeader, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let panel = self.props.mode == DetailMode::Panel;
        let mut top = div().flex().flex_col().gap(u(if panel { 8. } else { 10. }));
        if !panel {
            top = top.child(self.render_identity_row(header, cx));
        }
        top = top
            .child(
                div()
                    .id("detail-title")
                    .line_clamp(2)
                    .text_ellipsis()
                    .semibold()
                    .leading(theme.leading.tight)
                    .text_color(theme.colors.content)
                    .text_px(if panel { 18. } else { 20. })
                    .tooltip(tooltip(self.item.title.clone()))
                    .child(self.item.title.clone()),
            )
            .child(self.render_meta_row(header, cx))
            .children(self.render_related(cx))
            .child(self.render_actions(header, cx));
        let mut inner = div()
            .mx_auto()
            .flex()
            .flex_col()
            .w_full()
            .max_w(u(1024.))
            .gap(u(if panel { 8. } else { 10. }))
            .px(u(if panel { 16. } else { 32. }))
            .pt(u(if panel { 16. } else { 20. }))
            .child(top);
        if header.is_pr {
            inner = inner.child(self.render_tabs(cx));
        } else {
            inner = inner.pb(u(if panel { 16. } else { 20. }));
        }
        div()
            .relative()
            .flex_none()
            .border_b_1()
            .border_color(theme.colors.stroke)
            .child(inner)
            .into_any_element()
    }

    fn render_body(&mut self, header: &DetailHeader, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let panel = self.props.mode == DetailMode::Panel;
        let mut body = div()
            .mx_auto()
            .flex()
            .flex_col()
            .w_full()
            .max_w(u(1024.))
            .gap(u(if panel { 16. } else { 20. }))
            .px(u(if panel { 16. } else { 32. }))
            .py(u(if panel { 16. } else { 20. }));
        if !self.item.labels.is_empty() {
            body = body.child(
                div().flex().flex_wrap().gap(u(4.)).children(
                    self.item
                        .labels
                        .iter()
                        .map(|label| label_chip(label, false, cx)),
                ),
            );
        }
        if header.is_pr && self.tab == DetailTab::Code {
            let diff = &self.state.diff;
            body = if diff.loading {
                body.child(centered_loader("detail-diff-loading", cx))
            } else if let Some(error) = diff.error.clone() {
                body.child(
                    div()
                        .text_px(theme.text.body)
                        .text_color(theme.content(0.50))
                        .child(error),
                )
            } else if let Some((_, _, view)) = &self.diff_view {
                body.child(div().flex_1().min_h(u(320.)).h(u(640.)).child(view.clone()))
            } else {
                body.child(
                    div()
                        .text_px(theme.text.body)
                        .text_color(theme.content(0.45))
                        .child("No file changes"),
                )
            };
            return body.into_any_element();
        }
        if header.is_pr && self.tab == DetailTab::Checks {
            if let Some(view) = &self.checks_view {
                body = body.child(view.clone());
            }
            return body.into_any_element();
        }
        if self.state.details.loading {
            return body
                .child(centered_loader("detail-loading", cx))
                .into_any_element();
        }
        if let Some(error) = self.state.details.error.clone() {
            return body
                .child(
                    div()
                        .text_px(theme.text.body)
                        .text_color(theme.content(0.50))
                        .child(error),
                )
                .into_any_element();
        }
        let description = self
            .state
            .details
            .value
            .as_ref()
            .map(|details| details.body.clone())
            .filter(|text| !text.trim().is_empty());
        body = match description {
            Some(text) => {
                let view = self.markdown.view("body", &text, false, cx);
                body.child(div().min_w_0().child(view))
            }
            None => body.child(
                div()
                    .text_px(theme.text.body)
                    .text_color(theme.content(0.45))
                    .child("No description"),
            ),
        };
        let reply_mode = match self.item.provider {
            InboxProvider::Linear => Some(ReplyMode::Parent),
            InboxProvider::Github => Some(ReplyMode::Thread),
            _ => None,
        };
        let weak = cx.entity().downgrade();
        let props = CommentsProps {
            provider: self.item.provider,
            reply_mode,
            on_reply: Some(Rc::new(move |target: InboxReplyTarget, window, cx| {
                if let Some(view) = weak.upgrade() {
                    view.update(cx, |view, cx| view.reply_to(Some(target), window, cx));
                }
            })),
            now: self.services.now_ms(),
        };
        let thread = self.state.thread.clone();
        if let Some(comments) = inbox_comments(&thread, &props, &mut self.markdown, cx) {
            body = body.child(comments);
        }
        let weak = cx.entity().downgrade();
        let cancel_weak = weak.clone();
        body = body.child(
            div()
                .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    let keys = &event.keystroke;
                    if keys.key == "enter" && (keys.modifiers.platform || keys.modifiers.control) {
                        cx.stop_propagation();
                        this.submit_comment(window, cx);
                    }
                }))
                .child(comment_form(
                    CommentForm {
                        field: &self.comment_field,
                        draft: &self.draft,
                        reply_to: self.state.reply_to.as_ref(),
                        posting: self.state.posting,
                        error: self.state.post_error.as_deref(),
                        on_cancel_reply: Rc::new(move |window, cx| {
                            if let Some(view) = cancel_weak.upgrade() {
                                view.update(cx, |view, cx| view.reply_to(None, window, cx));
                            }
                        }),
                        on_submit: Rc::new(move |window, cx| {
                            if let Some(view) = weak.upgrade() {
                                view.update(cx, |view, cx| view.submit_comment(window, cx));
                            }
                        }),
                    },
                    cx,
                )),
        );
        body.into_any_element()
    }

    /// Replies to a comment, or `None` for a new top-level comment.
    pub fn reply_to(
        &mut self,
        target: Option<InboxReplyTarget>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let replying = target.is_some();
        self.data.set_reply_to(target, cx);
        if replying && crate::autofocus() {
            self.comment_field
                .update(cx, |field, cx| field.focus(window, cx));
        }
    }
}

impl Render for InboxDetailView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let replying = self.state.reply_to.is_some();
        if replying != self.placeholder_replying {
            self.placeholder_replying = replying;
            self.comment_field.update(cx, |field, cx| {
                field.set_placeholder(comment_placeholder(replying), window, cx)
            });
        }
        let header = self.header();
        let panel = self.props.mode == DetailMode::Panel;
        let header_el = self.render_header(&header, cx);
        let body_el = self.render_body(&header, cx);
        self.markdown.sweep();
        let root = div().flex().flex_col().size_full().min_h_0().min_w_0();
        if panel {
            let identity = self.render_identity_row(&header, cx);
            return root
                .child(identity)
                .child(
                    div()
                        .id("detail-scroll")
                        .flex_1()
                        .min_h_0()
                        .min_w_0()
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll)
                        .child(header_el)
                        .child(body_el),
                )
                .into_any_element();
        }
        root.child(header_el)
            .child(
                div()
                    .id("detail-scroll")
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child(body_el),
            )
            .into_any_element()
    }
}

/// The empty detail pane: "Select an inbox item".
pub fn empty_detail(cx: &App) -> AnyElement {
    let theme = Theme::of(cx);
    div()
        .flex()
        .size_full()
        .flex_col()
        .items_center()
        .justify_center()
        .px(u(24.))
        .child(
            icon(IconName::Inbox)
                .mb(u(12.))
                .size(u(24.))
                .text_color(theme.content(0.30)),
        )
        .child(
            div()
                .text_px(theme.text.body)
                .text_color(theme.content(0.45))
                .child("Select an inbox item"),
        )
        .into_any_element()
}

/// Exposed so the list can reuse it.
pub fn checks_tab_label(overall: &ChecksOverall) -> String {
    PrChecksTab::label(overall)
}

/// An `ElementId` from a string key.
pub fn key_id(prefix: &str, key: &str) -> ElementId {
    ElementId::Name(format!("{prefix}:{key}").into())
}

#[cfg(test)]
#[path = "detail_appearance_tests.rs"]
mod appearance_tests;
