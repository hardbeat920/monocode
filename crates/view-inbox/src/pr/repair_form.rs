//! Port of src/features/inbox/ui/CheckRepairForm.tsx: the "Fix with AI"
//! popover. It picks a project chat (or a new one), loads each selected
//! job's details three at a time, and hands the evidence to the repair
//! callback.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    App, AppContext as _, AsyncApp, Context, ElementId, Entity, EventEmitter, FocusHandle,
    Focusable, InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Render,
    SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Task, WeakEntity,
    Window, div, prelude::FluentBuilder as _,
};
use gpui_component::input::{Enter, Input, InputEvent, InputState, MoveDown, MoveUp};
use monocode_ui::widgets::popover_frame;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use crate::data::{
    CiEvidenceDetails, CiRepairEvidence, CiRepairStart, DataTask, GithubPrCheck, InboxServices,
    RelatedSession,
};
use crate::model::github_actions_job_id;
use crate::style::{closed_ink, loader};

/// Starts a repair: the evidence and the chosen chat (`None` for a new
/// project chat).
pub type RepairStartFn = Rc<dyn Fn(CiRepairStart, Option<String>, &mut App) -> DataTask<()>>;

/// Opens a repair chat.
pub type OpenSessionFn = Rc<dyn Fn(&str, &mut Window, &mut App)>;

/// `CheckRepair`: what the checks panel needs to start and follow repairs.
#[derive(Clone)]
pub struct CheckRepair {
    pub number: i64,
    pub sessions: Vec<RelatedSession>,
    pub on_start: RepairStartFn,
    pub on_open_session: Option<OpenSessionFn>,
}

/// The form closed: Escape, the close button, a click outside, or a
/// finished start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CloseRepairForm;

pub struct CheckRepairForm {
    services: Rc<dyn InboxServices>,
    checks: Vec<GithubPrCheck>,
    head_oid: String,
    cwd: String,
    repo: String,
    repair: CheckRepair,
    blocked: bool,
    session_id: String,
    query: String,
    active: usize,
    busy: bool,
    error: Option<String>,
    search: Entity<InputState>,
    focus: FocusHandle,
    animate: bool,
    _start: Option<Task<()>>,
    _search_events: Subscription,
}

impl EventEmitter<CloseRepairForm> for CheckRepairForm {}

impl Focusable for CheckRepairForm {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// A choice in the chat list: `""` is a new project chat.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Choice {
    id: String,
    title: String,
}

impl CheckRepairForm {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        services: Rc<dyn InboxServices>,
        checks: Vec<GithubPrCheck>,
        head_oid: String,
        cwd: String,
        repo: String,
        repair: CheckRepair,
        blocked: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search chats..."));
        if crate::autofocus() {
            search.update(cx, |search, cx| search.focus(window, cx));
        }
        let search_events = cx.subscribe_in(&search, window, |this, search, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                this.query = search.read(cx).value().to_string();
                this.active = 0;
                cx.notify();
            }
        });
        Self {
            services,
            checks,
            head_oid,
            cwd,
            repo,
            repair,
            blocked,
            session_id: String::new(),
            query: String::new(),
            active: 0,
            busy: false,
            error: None,
            search,
            focus: cx.focus_handle(),
            animate: true,
            _start: None,
            _search_events: search_events,
        }
    }

    /// Turns the open animation off, for screenshots.
    pub fn set_animate(&mut self, animate: bool) {
        self.animate = animate;
    }

    /// The checks panel is refreshing or stale: starting waits.
    pub fn set_blocked(&mut self, blocked: bool, cx: &mut Context<Self>) {
        if self.blocked != blocked {
            self.blocked = blocked;
            cx.notify();
        }
    }

    pub fn checks(&self) -> &[GithubPrCheck] {
        &self.checks
    }

    pub fn busy(&self) -> bool {
        self.busy
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// The chosen chat id, empty for a new project chat.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    fn choices(&self) -> Vec<Choice> {
        let needle = self.query.trim().to_lowercase();
        let mut choices = vec![Choice {
            id: String::new(),
            title: "New project chat".into(),
        }];
        choices.extend(
            self.repair
                .sessions
                .iter()
                .filter(|session| {
                    let title = if session.title.is_empty() {
                        "Untitled chat"
                    } else {
                        session.title.as_str()
                    };
                    title.to_lowercase().contains(&needle)
                })
                .map(|session| Choice {
                    id: session.id.clone(),
                    title: session.title.clone(),
                }),
        );
        choices
    }

    fn selected_title(&self) -> String {
        if self.session_id.is_empty() {
            return "New project chat".into();
        }
        self.repair
            .sessions
            .iter()
            .find(|session| session.id == self.session_id)
            .map(|session| session.title.clone())
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| "Untitled chat".into())
    }

    /// Typing in the search field, for tests.
    pub fn set_query(&mut self, query: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.search.update(cx, |search, cx| {
            search.set_value(query.to_string(), window, cx)
        });
        self.query = query.to_string();
        self.active = 0;
        cx.notify();
    }

    /// ArrowDown or ArrowUp in the search field.
    pub fn step(&mut self, down: bool, cx: &mut Context<Self>) {
        let count = self.choices().len();
        if count == 0 {
            return;
        }
        self.active = if down {
            (self.active + 1) % count
        } else {
            (self.active + count - 1) % count
        };
        cx.notify();
    }

    /// Enter in the search field: pick the highlighted chat.
    pub fn pick_active(&mut self, cx: &mut Context<Self>) {
        let choices = self.choices();
        let index = self.active.min(choices.len().saturating_sub(1));
        self.session_id = choices
            .get(index)
            .map(|choice| choice.id.clone())
            .unwrap_or_default();
        cx.notify();
    }

    pub fn pick(&mut self, id: &str, cx: &mut Context<Self>) {
        self.session_id = id.to_string();
        cx.notify();
    }

    fn dismiss(&mut self, cx: &mut Context<Self>) {
        cx.emit(CloseRepairForm);
    }

    /// "Start fix": load each job's details, three at a time, then start.
    pub fn start(&mut self, cx: &mut Context<Self>) {
        if self.busy || self.blocked {
            return;
        }
        self.busy = true;
        self.error = None;
        cx.notify();
        let services = self.services.clone();
        let checks = self.checks.clone();
        let cwd = self.cwd.clone();
        let repo = self.repo.clone();
        let head_oid = self.head_oid.clone();
        let number = self.repair.number;
        let on_start = self.repair.on_start.clone();
        let session_id = (!self.session_id.is_empty()).then(|| self.session_id.clone());
        self._start = Some(
            cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
                let evidence = load_evidence(services, &checks, &cwd, &repo, cx.clone()).await;
                let Some(evidence) = evidence else {
                    return;
                };
                if this.upgrade().is_none() {
                    return;
                }
                let start = CiRepairStart {
                    repo,
                    number,
                    head_oid,
                    evidence,
                };
                let task = cx.update(|cx| on_start(start, session_id, cx));
                let result = task.await;
                let _ = this.update(cx, |this, cx| {
                    this.busy = false;
                    match result {
                        Ok(()) => cx.emit(CloseRepairForm),
                        Err(error) => this.error = Some(error),
                    }
                    cx.notify();
                });
            }),
        );
    }
}

/// Loads every check's job details with three workers, keeping the order.
/// `None` when the app went away.
async fn load_evidence(
    services: Rc<dyn InboxServices>,
    checks: &[GithubPrCheck],
    cwd: &str,
    repo: &str,
    cx: AsyncApp,
) -> Option<Vec<CiRepairEvidence>> {
    let next = Rc::new(Cell::new(0usize));
    let slots: Rc<std::cell::RefCell<Vec<Option<CiRepairEvidence>>>> =
        Rc::new(std::cell::RefCell::new(vec![None; checks.len()]));
    let worker = |cx: AsyncApp| {
        let next = next.clone();
        let slots = slots.clone();
        let services = services.clone();
        async move {
            loop {
                let index = next.get();
                if index >= checks.len() {
                    return true;
                }
                next.set(index + 1);
                let check = checks[index].clone();
                let details = match github_actions_job_id(check.url.as_deref(), repo) {
                    Some(job_id) => {
                        let task =
                            cx.update(|cx| services.fetch_check_details(cwd, repo, &job_id, cx));
                        Some(match task.await {
                            Ok(details) => CiEvidenceDetails::Full(details),
                            Err(_) => CiEvidenceDetails::Notice(
                                "Job details unavailable. Inspect the check URL for logs.".into(),
                            ),
                        })
                    }
                    None => None,
                };
                slots.borrow_mut()[index] = Some(CiRepairEvidence { check, details });
            }
        }
    };
    let workers: Vec<_> = (0..checks.len().min(3))
        .map(|_| worker(cx.clone()))
        .collect();
    let finished = futures::future::join_all(workers).await;
    if finished.iter().any(|ok| !ok) {
        return None;
    }
    slots.borrow_mut().drain(..).collect::<Option<Vec<_>>>()
}

impl Render for CheckRepairForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let choices = self.choices();
        let active = self.active.min(choices.len().saturating_sub(1));
        let busy = self.busy;
        let subject = if self.checks.len() == 1 {
            self.checks[0].name.clone()
        } else {
            format!("{} failed checks", self.checks.len())
        };
        let names = self
            .checks
            .iter()
            .map(|check| check.name.clone())
            .collect::<Vec<_>>()
            .join(", ");
        let close_hover = theme.content(0.10);
        let header = div()
            .flex()
            .flex_none()
            .items_start()
            .gap(u(10.))
            .px(u(14.))
            .pb(u(12.))
            .pt(u(14.))
            .child(
                icon(IconName::Sparkles)
                    .mt(u(2.))
                    .size(u(16.))
                    .text_color(theme.content(0.65)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_px(theme.text.body)
                            .medium()
                            .line_height(u(20.))
                            .text_color(theme.colors.content)
                            .child("Fix with AI"),
                    )
                    .child(
                        div()
                            .id("repair-form-subject")
                            .mt(u(2.))
                            .truncate()
                            .text_px(theme.text.caption)
                            .line_height(u(16.))
                            .text_color(theme.content(0.45))
                            .tooltip(monocode_ui::widgets::tooltip(names))
                            .child(format!("{subject} · PR #{}", self.repair.number)),
                    ),
            )
            .child(
                div()
                    .id("repair-form-close")
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .size(u(24.))
                    .rounded(u(theme.radius.md))
                    .hover(move |s| s.bg(close_hover))
                    .on_click(cx.listener(|this, _, _, cx| this.dismiss(cx)))
                    .child(
                        icon(IconName::X)
                            .size(u(14.))
                            .text_color(theme.content(0.40)),
                    ),
            );
        let search = div()
            .mx(u(6.))
            .flex()
            .flex_none()
            .h(u(36.))
            .items_center()
            .gap(u(8.))
            .rounded(u(theme.radius.md))
            .bg(theme.content(0.05))
            .px(u(10.))
            .child(
                icon(IconName::Search)
                    .size(u(14.))
                    .text_color(theme.content(0.40)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h(u(20.))
                    .flex()
                    .items_center()
                    .text_px(theme.text.label)
                    .text_color(theme.colors.content)
                    .capture_action(cx.listener(|this: &mut Self, _: &MoveDown, _, cx| {
                        cx.stop_propagation();
                        this.step(true, cx);
                    }))
                    .capture_action(cx.listener(|this: &mut Self, _: &MoveUp, _, cx| {
                        cx.stop_propagation();
                        this.step(false, cx);
                    }))
                    .capture_action(cx.listener(|this: &mut Self, _: &Enter, _, cx| {
                        cx.stop_propagation();
                        this.pick_active(cx);
                    }))
                    .child(
                        Input::new(&self.search)
                            .appearance(false)
                            .disabled(busy)
                            .h_full()
                            .p_0()
                            .text_px(theme.text.label),
                    ),
            );
        let selected_id = self.session_id.clone();
        let mut list = div()
            .id("repair-form-chats")
            .my(u(6.))
            .min_h_0()
            .max_h(u(224.))
            .overflow_y_scroll()
            .px(u(6.));
        for (index, choice) in choices.iter().enumerate() {
            let selected = selected_id == choice.id;
            let title: SharedString = if choice.title.is_empty() {
                "Untitled chat".into()
            } else {
                choice.title.clone().into()
            };
            let hover_bg = theme.content(0.05);
            let ink = theme.colors.content;
            let id = choice.id.clone();
            let mut row = div()
                .id(ElementId::NamedInteger("repair-chat".into(), index as u64))
                .flex()
                .h(u(36.))
                .w_full()
                .items_center()
                .gap(u(10.))
                .rounded(u(theme.radius.lg))
                .px(u(10.))
                .text_px(theme.text.label)
                .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                    if *hovered && this.active != index {
                        this.active = index;
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !this.busy {
                        this.pick(&id, cx);
                    }
                }))
                .child(
                    icon(if choice.id.is_empty() {
                        IconName::Plus
                    } else {
                        IconName::MessageSquare
                    })
                    .size(u(14.))
                    .text_color(theme.content(0.50)),
                )
                .child(div().flex_1().min_w_0().truncate().child(title))
                .when(selected, |row| {
                    row.child(
                        icon(IconName::Check)
                            .size(u(14.))
                            .text_color(theme.content(0.75)),
                    )
                });
            row = if index == active {
                row.bg(theme.colors.selection).text_color(ink)
            } else {
                row.text_color(theme.content(0.70))
                    .hover(move |s| s.bg(hover_bg).text_color(ink))
            };
            if busy {
                row = row.opacity(0.5);
            }
            list = list.child(row);
        }
        if choices.len() == 1 && !self.query.trim().is_empty() {
            list = list.child(
                div()
                    .px(u(10.))
                    .py(u(12.))
                    .text_px(theme.text.label)
                    .text_color(theme.content(0.45))
                    .child("No matching chats"),
            );
        }
        let start_disabled = busy || self.blocked;
        let selection_hover = theme.colors.selection_hover;
        let mut start = div()
            .id("repair-form-start")
            .flex()
            .flex_none()
            .h(u(28.))
            .items_center()
            .gap(u(6.))
            .rounded(u(theme.radius.md))
            .bg(theme.colors.selection)
            .px(u(10.))
            .text_px(theme.text.label)
            .medium()
            .text_color(theme.colors.content);
        if start_disabled {
            start = start.opacity(0.5);
        } else {
            start = start
                .hover(move |s| s.bg(selection_hover))
                .on_click(cx.listener(|this, _, _, cx| this.start(cx)));
        }
        start = if busy {
            start
                .child(loader("repair-form-busy", 14., theme.colors.content))
                .child("Preparing...")
        } else {
            start.child("Start fix").child(
                icon(IconName::ChevronRight)
                    .size(u(12.))
                    .text_color(theme.colors.content),
            )
        };
        let selected_title = self.selected_title();
        let body = div()
            .id("repair-form")
            .key_context("CheckRepairForm")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    cx.stop_propagation();
                    this.dismiss(cx);
                }
            }))
            .on_mouse_down_out(cx.listener(|this, _, _, cx| this.dismiss(cx)))
            .flex()
            .flex_col()
            .min_h_0()
            .overflow_hidden()
            .child(header)
            .child(search)
            .child(list)
            .when_some(self.error.clone(), |body, error| {
                body.child(
                    div()
                        .px(u(14.))
                        .pb(u(12.))
                        .text_px(theme.text.label)
                        .line_height(u(16.))
                        .text_color(closed_ink())
                        .child(error),
                )
            })
            .when(self.blocked && !busy, |body| {
                body.child(
                    div()
                        .px(u(14.))
                        .pb(u(12.))
                        .text_px(theme.text.label)
                        .line_height(u(16.))
                        .text_color(theme.content(0.55))
                        .child("Wait for the latest checks before starting a fix."),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(u(12.))
                    .border_t_1()
                    .border_color(theme.colors.stroke)
                    .px(u(12.))
                    .py(u(10.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_px(theme.text.caption)
                            .line_height(u(16.))
                            .child(
                                div()
                                    .truncate()
                                    .text_color(theme.content(0.65))
                                    .child(selected_title),
                            )
                            .child(
                                div()
                                    .text_color(theme.content(0.35))
                                    .child("CI details included"),
                            ),
                    )
                    .child(start),
            );
        popover_frame("check-repair-form")
            .width(320.)
            .max_height(460.)
            .animate(self.animate)
            .child(body)
    }
}
