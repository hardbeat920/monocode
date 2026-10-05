//! Port of src/features/orchestration/ui/OrchestrationSidebarAgents.tsx:
//! the agents a lead's sidebar card lists, each expandable for its model and
//! actions, plus the paused run's Resume panel.

use std::collections::BTreeSet;
use std::rc::Rc;
use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Task, WeakEntity,
    Window, div, px,
};
use monocode_core::HarnessId;
use monocode_core::models::ModelCatalog;
use monocode_ui::widgets::{spinner, tooltip};
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use super::actions::{
    OrchestrationActions, OrchestrationRunStatus, OrchestrationRunView, OrchestrationRuns,
    OrchestrationSummary, OrchestrationTaskStatus, OrchestrationWorkerDetail, OrchestrationWorkers,
    ResumeBlocker, orchestration_task_label,
};
use super::parts::{eid, harness_icon};

/// How a row's status reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentTone {
    /// Needs input, failed, blocked, or interrupted (amber).
    Attention,
    /// Running in a live run (accent, with a spinner).
    Working,
    /// Completed (emerald, with a check).
    Done,
    Idle,
}

/// One agent row as drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct AgentRow {
    pub session_id: String,
    pub title: String,
    pub harness: HarnessId,
    /// The catalog name, or the saved id when this window's catalog lacks it.
    pub model: String,
    pub label: &'static str,
    pub tone: AgentTone,
    pub open: bool,
    /// The live task's error.
    pub error: Option<String>,
    /// The live task id, when it can still be cancelled.
    pub cancel_task_id: Option<String>,
}

/// The paused run's panel.
#[derive(Clone, Debug, PartialEq)]
pub struct PausedPanel {
    pub message: &'static str,
    pub blocker: Option<ResumeBlocker>,
    pub can_open_blocker: bool,
    pub resume_disabled: bool,
    pub resume_title: String,
}

/// The sidebar card's agent list.
pub struct OrchestrationSidebarAgents {
    lead_id: String,
    summary: OrchestrationSummary,
    runs: Rc<dyn OrchestrationRuns>,
    workers: Rc<dyn OrchestrationWorkers>,
    actions: Option<Rc<dyn OrchestrationActions>>,
    catalog: Arc<ModelCatalog>,
    error: Option<String>,
    pending: bool,
    /// Rows expand independently, so several agents can be watched side by side.
    expanded: BTreeSet<String>,
    revealed: Option<String>,
    operation: Option<Task<()>>,
    _runs: Subscription,
}

impl OrchestrationSidebarAgents {
    pub fn new(
        lead_id: impl Into<String>,
        summary: OrchestrationSummary,
        runs: Rc<dyn OrchestrationRuns>,
        workers: Rc<dyn OrchestrationWorkers>,
        cx: &mut Context<Self>,
    ) -> Self {
        let weak = cx.entity().downgrade();
        let subscription = runs.observe(
            Box::new(move |cx| {
                weak.update(cx, |_, cx| cx.notify()).ok();
            }),
            cx,
        );
        let mut this = Self {
            lead_id: lead_id.into(),
            summary,
            runs,
            workers,
            actions: None,
            catalog: Arc::new(ModelCatalog::new()),
            error: None,
            pending: false,
            expanded: BTreeSet::new(),
            revealed: None,
            operation: None,
            _runs: subscription,
        };
        this.sync_revealed(cx);
        this
    }

    pub fn set_summary(&mut self, summary: OrchestrationSummary, cx: &mut Context<Self>) {
        self.summary = summary;
        cx.notify();
    }

    /// The `OrchestrationActions` context, when the card has one.
    pub fn set_actions(
        &mut self,
        actions: Option<Rc<dyn OrchestrationActions>>,
        cx: &mut Context<Self>,
    ) {
        self.actions = actions;
        cx.notify();
    }

    pub fn set_catalog(&mut self, catalog: Arc<ModelCatalog>, cx: &mut Context<Self>) {
        self.catalog = catalog;
        cx.notify();
    }

    /// Revealing a worker from its toast opens that row without closing
    /// others. Call when the workers' selection changed.
    pub fn sync_revealed(&mut self, cx: &mut Context<Self>) {
        let revealed = self.workers.selected_id(cx);
        if revealed != self.revealed {
            if let Some(id) = &revealed {
                self.expanded.insert(id.clone());
            }
            self.revealed = revealed;
            cx.notify();
        }
    }

    /// A saved run has no live entry, so the card stays read-only after a reload.
    fn run(&self, cx: &gpui::App) -> Option<OrchestrationRunView> {
        self.runs
            .runs(cx)
            .into_iter()
            .find(|run| run.lead_id == self.lead_id)
    }

    /// The rows as drawn.
    pub fn rows(&self, cx: &gpui::App) -> Vec<AgentRow> {
        let run = self.run(cx);
        let live_run = self.summary.live == Some(true);
        self.summary
            .tasks
            .iter()
            .map(|task| {
                // Something waiting on an answer opens itself; it cannot be missed.
                let open = self.expanded.contains(&task.session_id) || task.needs_input();
                let live = run.as_ref().and_then(|run| {
                    run.tasks
                        .iter()
                        .find(|entry| entry.session_id == task.session_id)
                });
                let label = orchestration_task_label(task, &self.summary);
                let working = live_run && task.status == "running" && !task.needs_input();
                let attention = task.needs_input()
                    || matches!(task.status.as_str(), "failed" | "blocked" | "interrupted");
                let tone = if attention {
                    AgentTone::Attention
                } else if working {
                    AgentTone::Working
                } else if task.status == "completed" {
                    AgentTone::Done
                } else {
                    AgentTone::Idle
                };
                // A saved provider model may not be in this window's catalog
                // yet. Keep its identity instead of the harness default.
                let model = self
                    .catalog
                    .find_model(&task.model)
                    .map(|model| model.name.clone())
                    .unwrap_or_else(|| task.model.clone());
                AgentRow {
                    session_id: task.session_id.clone(),
                    title: task.title.clone(),
                    harness: task.harness,
                    model,
                    label,
                    tone,
                    open,
                    error: live.and_then(|live| live.error.clone()),
                    cancel_task_id: live
                        .filter(|live| {
                            matches!(
                                live.status,
                                OrchestrationTaskStatus::Queued | OrchestrationTaskStatus::Running
                            )
                        })
                        .map(|live| live.id.clone()),
                }
            })
            .collect()
    }

    /// The session ids of the open rows.
    pub fn open_ids(&self, cx: &gpui::App) -> Vec<String> {
        self.rows(cx)
            .into_iter()
            .filter(|row| row.open)
            .map(|row| row.session_id)
            .collect()
    }

    /// The paused run's panel, when the run is paused.
    pub fn paused_panel(&self, cx: &gpui::App) -> Option<PausedPanel> {
        let run = self.run(cx)?;
        if run.status != OrchestrationRunStatus::Paused {
            return None;
        }
        let stopping = run.tasks.iter().any(|task| {
            matches!(
                task.status,
                OrchestrationTaskStatus::Running | OrchestrationTaskStatus::Cancelling
            )
        });
        let blocker = self.runs.resume_blocker(&self.lead_id, cx);
        let lead_busy = self.runs.resume_lead_busy(&self.lead_id, cx);
        let message = if stopping {
            "Stopping interrupted work before this run can resume."
        } else if lead_busy {
            "Waiting for the lead's interrupted turn to finish before this run can resume."
        } else {
            "Resume continues interrupted workers from their retained checkouts and starts queued work. Policy-blocked tasks stay stopped for review."
        };
        let resume_title = if stopping {
            "Wait for interrupted agents to stop".to_string()
        } else if lead_busy {
            "Wait for the lead's interrupted turn to finish".to_string()
        } else if let Some(blocker) = &blocker {
            format!(
                "Stop {} before resuming",
                if blocker.title.is_empty() {
                    "the other conversation"
                } else {
                    &blocker.title
                }
            )
        } else {
            "Continue interrupted and queued work".to_string()
        };
        Some(PausedPanel {
            message,
            can_open_blocker: blocker.is_some() && self.actions.is_some(),
            resume_disabled: self.pending || stopping || lead_busy || blocker.is_some(),
            blocker,
            resume_title,
        })
    }

    /// The error under the list: this card's last failure, else the run's.
    pub fn error(&self, cx: &gpui::App) -> Option<String> {
        self.error
            .clone()
            .or_else(|| self.run(cx).and_then(|run| run.error))
    }

    /// Expands or collapses a row. Collapsing the revealed row clears the
    /// reveal; expanding is not a request to open the lead's tab.
    pub fn toggle(&mut self, session_id: &str, cx: &mut Context<Self>) {
        let open = self
            .rows(cx)
            .iter()
            .any(|row| row.session_id == session_id && row.open);
        if open {
            self.expanded.remove(session_id);
        } else {
            self.expanded.insert(session_id.to_string());
        }
        if open && self.revealed.as_deref() == Some(session_id) {
            self.workers.inspect(None, cx);
        }
        cx.notify();
    }

    /// See details: open this agent beside the orchestrator.
    pub fn see_details(&mut self, session_id: &str, cx: &mut Context<Self>) {
        let Some(task) = self
            .summary
            .tasks
            .iter()
            .find(|task| task.session_id == session_id)
        else {
            return;
        };
        let worker = OrchestrationWorkerDetail {
            session_id: task.session_id.clone(),
            lead_id: self.lead_id.clone(),
            title: task.title.clone(),
            harness: task.harness,
        };
        self.workers.open_details(worker, cx);
    }

    fn perform(
        &mut self,
        operation: impl FnOnce(&mut Self, &mut Context<Self>) -> Task<Result<(), String>>,
        cx: &mut Context<Self>,
    ) {
        self.pending = true;
        self.error = None;
        let task = operation(self, cx);
        self.operation = Some(cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                this.pending = false;
                if let Err(error) = result {
                    this.error = Some(error);
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    pub fn cancel_task(&mut self, task_id: &str, cx: &mut Context<Self>) {
        let task_id = task_id.to_string();
        self.perform(
            move |this, cx| this.runs.cancel_task(&this.lead_id, &task_id, cx),
            cx,
        );
    }

    pub fn resume(&mut self, cx: &mut Context<Self>) {
        let Some(run) = self.run(cx) else {
            return;
        };
        self.perform(
            move |this, cx| {
                this.runs
                    .start(&this.lead_id, &run.allowed_harnesses, run.max_workers, cx)
            },
            cx,
        );
    }

    pub fn open_blocker(&mut self, cx: &mut Context<Self>) {
        let Some(blocker) = self.runs.resume_blocker(&self.lead_id, cx) else {
            return;
        };
        if let Some(actions) = &self.actions {
            actions.open(&blocker.id, cx);
        }
    }

    fn render_row(&self, row: AgentRow, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let key = row.session_id.clone();
        let tone_color = match row.tone {
            AgentTone::Attention => theme.colors.warning,
            AgentTone::Working => theme.colors.accent,
            AgentTone::Done => theme.colors.success,
            AgentTone::Idle => theme.content(0.45),
        };
        let status_icon = match row.tone {
            AgentTone::Attention => Some(
                icon(IconName::CircleAlert)
                    .size(u(12.))
                    .text_color(tone_color)
                    .into_any_element(),
            ),
            AgentTone::Working => Some(
                spinner(eid(&key, "spinner"))
                    .color(theme.colors.accent)
                    .into_any_element(),
            ),
            AgentTone::Done => Some(
                icon(IconName::Check)
                    .size(u(12.))
                    .text_color(tone_color)
                    .into_any_element(),
            ),
            AgentTone::Idle => None,
        };
        let slot = if row.open {
            div()
                .size(u(14.))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    icon(IconName::ChevronDown)
                        .size(u(12.))
                        .text_color(theme.content(0.45)),
                )
        } else {
            // One slot: the chevron stands in for the harness mark under the
            // pointer.
            div()
                .relative()
                .size(u(14.))
                .child(
                    div()
                        .absolute()
                        .size_full()
                        .opacity(0.75)
                        .group_hover("orchestration-agent", |style| style.opacity(0.))
                        .child(harness_icon(row.harness, 14.)),
                )
                .child(
                    div()
                        .absolute()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .opacity(0.)
                        .group_hover("orchestration-agent", |style| style.opacity(1.))
                        .child(
                            icon(IconName::ChevronRight)
                                .size(u(12.))
                                .text_color(theme.content(0.45)),
                        ),
                )
        };
        let hover = theme.content(0.10);
        let toggle_key = key.clone();
        let header = div()
            .id(eid(&key, "agent"))
            .group("orchestration-agent")
            .flex()
            .w_full()
            .min_w_0()
            .items_center()
            .gap(u(6.))
            .rounded(u(theme.radius.md))
            .px(u(8.))
            .py(u(6.))
            .when(!row.open, |el| el.hover(move |style| style.bg(hover)))
            .tooltip(tooltip(format!(
                "{} · {} · {} · {}",
                row.title,
                row.harness.title(),
                row.model,
                row.label
            )))
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.toggle(&toggle_key, cx);
            }))
            .child(slot)
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .text_px(12.)
                    .leading(theme.leading.snug)
                    .text_color(theme.content(0.80))
                    .child(row.title.clone()),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(u(4.))
                    .text_px(11.)
                    .text_color(tone_color)
                    .children(status_icon)
                    .child(row.label),
            );
        let mut container = div()
            .rounded(u(theme.radius.md))
            .when(row.open, |el| el.bg(theme.colors.selection))
            .child(header);
        if row.open {
            let solid = |id: &str, label: &'static str, disabled: bool| {
                let hover_bg = theme.content(0.25);
                let ink = theme.colors.content;
                div()
                    .id(eid(&key, id))
                    .rounded(u(theme.radius.sm))
                    .bg(theme.content(0.15))
                    .px(u(6.))
                    .py(u(2.))
                    .text_px(11.)
                    .text_color(theme.content(0.75))
                    .when(disabled, |el| el.opacity(0.35))
                    .when(!disabled, |el| {
                        el.hover(move |style| style.bg(hover_bg).text_color(ink))
                    })
                    .child(label)
            };
            let mut buttons = div().flex().flex_wrap().items_center().gap(u(4.));
            if self.workers.can_open_details() {
                let details_key = key.clone();
                buttons = buttons.child(
                    solid("details", "See details", false)
                        .tooltip(tooltip("Open this agent beside the orchestrator"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.see_details(&details_key, cx);
                        })),
                );
            }
            if let Some(task_id) = row.cancel_task_id.clone() {
                let pending = self.pending;
                buttons = buttons.child(solid("cancel", "Cancel task", pending).on_click(
                    cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        if !this.pending {
                            this.cancel_task(&task_id, cx);
                        }
                    }),
                ));
            }
            container = container.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(u(8.))
                    .pb(u(12.))
                    .pl(u(26.))
                    .pr(u(8.))
                    .child(
                        div()
                            .flex()
                            .min_w_0()
                            .items_center()
                            .gap(u(6.))
                            .text_px(11.)
                            .text_color(theme.content(0.45))
                            .child(harness_icon(row.harness, 14.))
                            .child(div().min_w_0().truncate().child(row.model.clone())),
                    )
                    .children(row.error.clone().map(|error| {
                        div()
                            .text_px(11.)
                            .text_color(theme.colors.danger)
                            .child(error)
                    }))
                    .child(buttons),
            );
        }
        container.into_any_element()
    }
}

impl Render for OrchestrationSidebarAgents {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_revealed(cx);
        let theme = Theme::of(cx).clone();
        let rows = self.rows(cx);
        let total = self.summary.tasks.len();
        let done = self
            .summary
            .tasks
            .iter()
            .filter(|task| task.status == "completed")
            .count();
        let mut list = div()
            .id("orchestrated-agents")
            .flex()
            .flex_col()
            .gap(px(1.))
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation());
        for row in rows {
            list = list.child(self.render_row(row, &theme, cx));
        }
        let error = self.error(cx);
        let paused = self.paused_panel(cx);
        let action = |id: &'static str, label: &'static str, disabled: bool| {
            let hover = theme.content(0.10);
            let ink = theme.colors.content;
            div()
                .id(id)
                .rounded(u(theme.radius.sm))
                .px(u(6.))
                .py(u(2.))
                .text_px(11.)
                .text_color(theme.content(0.55))
                .when(disabled, |el| el.opacity(0.35))
                .when(!disabled, |el| {
                    el.hover(move |style| style.bg(hover).text_color(ink))
                })
                .child(label)
        };
        let paused = paused.map(|panel| {
            let mut buttons = div()
                .mr(u(-6.))
                .flex()
                .items_center()
                .justify_end()
                .gap(u(4.));
            if panel.can_open_blocker {
                let pending = self.pending;
                buttons = buttons.child(action("open-blocker", "Open blocker", pending).on_click(
                    cx.listener(|this, _, _, cx| {
                        if !this.pending {
                            this.open_blocker(cx);
                        }
                    }),
                ));
            }
            let disabled = panel.resume_disabled;
            buttons = buttons.child(
                action("resume", "Resume", disabled)
                    .tooltip(tooltip(panel.resume_title.clone()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !disabled {
                            this.resume(cx);
                        }
                    })),
            );
            div()
                .mt(u(6.))
                .flex()
                .flex_col()
                .gap(u(6.))
                .border_t_1()
                .border_color(theme.colors.stroke)
                .pt(u(6.))
                .child(
                    div()
                        .px(u(2.))
                        .text_px(11.)
                        .leading(theme.leading.relaxed)
                        .text_color(theme.content(0.45))
                        .child(panel.message),
                )
                .children(panel.blocker.as_ref().map(|blocker| {
                    div()
                        .px(u(2.))
                        .text_px(11.)
                        .leading(theme.leading.relaxed)
                        .text_color(theme.colors.warning)
                        .child(SharedString::from(format!(
                            "{} is still running in this project.",
                            if blocker.title.is_empty() {
                                "Another conversation"
                            } else {
                                &blocker.title
                            }
                        )))
                }))
                .child(buttons)
        });
        div()
            .relative()
            .mt(u(6.))
            .child(
                div()
                    .mb(u(2.))
                    .px(u(2.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_px(11.)
                    .text_color(theme.content(0.45))
                    .child(format!(
                        "{total} {}",
                        if total == 1 { "agent" } else { "agents" }
                    ))
                    .child(div().tabular().child(format!("{done}/{total} done"))),
            )
            // Offset by the rows' own padding so a chevron lands on the
            // card's content edge, under the harness icon of the header
            // above. No height cap and no scroller: the sidebar scrolls.
            .child(div().mx(u(-8.)).child(list))
            .children(error.map(|error| {
                div()
                    .py(u(4.))
                    .text_px(11.)
                    .text_color(theme.colors.danger)
                    .child(error)
            }))
            .children(paused)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::threads::actions::{OrchestrationSummaryTask, OrchestrationTaskView};
    use gpui::{App, AppContext as _, TestAppContext};
    use monocode_core::Extra;
    use monocode_core::models::AgentModel;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Fake {
        runs: RefCell<Vec<OrchestrationRunView>>,
        blocker: RefCell<Option<ResumeBlocker>>,
        selected: RefCell<Option<String>>,
        inspected: RefCell<Vec<Option<String>>>,
        details: RefCell<Vec<OrchestrationWorkerDetail>>,
        opened: RefCell<Vec<String>>,
        details_enabled: bool,
    }

    impl OrchestrationRuns for Fake {
        fn runs(&self, _: &App) -> Vec<OrchestrationRunView> {
            self.runs.borrow().clone()
        }
        fn observe(&self, _: Box<dyn Fn(&mut App)>, _: &mut App) -> Subscription {
            Subscription::new(|| {})
        }
        fn resume_blocker(&self, _: &str, _: &App) -> Option<ResumeBlocker> {
            self.blocker.borrow().clone()
        }
        fn cancel_task(&self, _: &str, _: &str, _: &mut App) -> Task<Result<(), String>> {
            Task::ready(Ok(()))
        }
        fn start(&self, _: &str, _: &[HarnessId], _: i64, _: &mut App) -> Task<Result<(), String>> {
            Task::ready(Ok(()))
        }
    }

    impl OrchestrationWorkers for Fake {
        fn selected_id(&self, _: &App) -> Option<String> {
            self.selected.borrow().clone()
        }
        fn inspect(&self, session_id: Option<&str>, _: &mut App) {
            *self.selected.borrow_mut() = session_id.map(str::to_string);
            self.inspected
                .borrow_mut()
                .push(session_id.map(str::to_string));
        }
        fn can_open_details(&self) -> bool {
            self.details_enabled
        }
        fn open_details(&self, worker: OrchestrationWorkerDetail, _: &mut App) {
            self.details.borrow_mut().push(worker);
        }
    }

    impl OrchestrationActions for Fake {
        fn update(
            &self,
            _: &str,
            _: &str,
            _: monocode_core::orchestration::OrchestrationProposal,
            _: &mut App,
        ) {
        }
        fn confirm(&self, _: &str, _: &str, _: &mut App) -> Task<Result<(), String>> {
            Task::ready(Ok(()))
        }
        fn retry(&self, _: &str, _: &str, _: &mut App) {}
        fn open(&self, session_id: &str, _: &mut App) {
            self.opened.borrow_mut().push(session_id.to_string());
        }
    }

    fn summary_task(id: &str, title: &str, needs_input: bool) -> OrchestrationSummaryTask {
        OrchestrationSummaryTask {
            session_id: id.into(),
            title: title.into(),
            harness: HarnessId::Codex,
            model: "codex:two".into(),
            status: "running".into(),
            needs_input: needs_input.then_some(true),
            extra: Extra::new(),
        }
    }

    fn summary(status: &str, tasks: Vec<OrchestrationSummaryTask>) -> OrchestrationSummary {
        OrchestrationSummary {
            status: status.into(),
            live: Some(true),
            tasks,
            extra: Extra::new(),
        }
    }

    fn catalog() -> Arc<ModelCatalog> {
        let mut catalog = ModelCatalog::new();
        catalog.set_harness_models(
            HarnessId::Codex,
            vec![AgentModel::new("codex:two", HarnessId::Codex, "Worker Two")],
        );
        Arc::new(catalog)
    }

    fn init(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            monocode_ui::init(monocode_ui::AppearanceSettings::default(), cx);
        });
    }

    fn mount(
        cx: &mut TestAppContext,
        fake: Rc<Fake>,
        summary: OrchestrationSummary,
        with_actions: bool,
    ) -> gpui::Entity<OrchestrationSidebarAgents> {
        cx.new(|cx| {
            let mut card =
                OrchestrationSidebarAgents::new("lead", summary, fake.clone(), fake.clone(), cx);
            card.set_catalog(catalog(), cx);
            if with_actions {
                card.set_actions(Some(fake.clone()), cx);
            }
            card
        })
    }

    #[gpui::test]
    fn shows_a_blocked_agent_as_the_leads_to_answer_not_the_users(cx: &mut TestAppContext) {
        init(cx);
        let fake = Rc::new(Fake::default());
        let card = mount(
            cx,
            fake,
            summary(
                "active",
                vec![
                    summary_task("worker", "UI worker", true),
                    summary_task("second", "Check worker", true),
                ],
            ),
            false,
        );
        card.read_with(cx, |card, cx| {
            let rows = card.rows(cx);
            assert_eq!(rows[0].title, "UI worker");
            // A blocked agent expands itself so its model is visible, but
            // the approval still belongs to the lead.
            assert!(rows[0].open);
            assert_eq!(rows[0].model, "Worker Two");
            assert_eq!(rows[0].label, "Needs input");
            assert_eq!(rows[0].tone, AgentTone::Attention);
        });
    }

    #[gpui::test]
    fn inspects_an_agent_without_taking_the_cards_click_or_its_tab(cx: &mut TestAppContext) {
        init(cx);
        let fake = Rc::new(Fake {
            details_enabled: true,
            ..Fake::default()
        });
        let card = mount(
            cx,
            fake.clone(),
            summary("active", vec![summary_task("worker", "UI worker", false)]),
            false,
        );
        card.update(cx, |card, cx| card.toggle("worker", cx));
        card.read_with(cx, |card, cx| assert_eq!(card.open_ids(cx), ["worker"]));
        // Expanding a row is not a request to open the lead's tab.
        assert!(fake.opened.borrow().is_empty());
        card.update(cx, |card, cx| card.see_details("worker", cx));
        assert_eq!(
            *fake.details.borrow(),
            [OrchestrationWorkerDetail {
                session_id: "worker".into(),
                lead_id: "lead".into(),
                title: "UI worker".into(),
                harness: HarnessId::Codex,
            }]
        );
    }

    #[gpui::test]
    fn expands_any_number_of_agents_at_once(cx: &mut TestAppContext) {
        init(cx);
        let fake = Rc::new(Fake::default());
        let tasks = ["one", "two", "three"]
            .iter()
            .map(|id| summary_task(id, &format!("Agent {id}"), false))
            .collect();
        let card = mount(cx, fake, summary("active", tasks), false);
        card.update(cx, |card, cx| {
            card.toggle("one", cx);
            card.toggle("three", cx);
        });
        card.read_with(cx, |card, cx| {
            assert_eq!(card.open_ids(cx), ["one", "three"])
        });
        // Collapsing one leaves the other where it was.
        card.update(cx, |card, cx| card.toggle("one", cx));
        card.read_with(cx, |card, cx| assert_eq!(card.open_ids(cx), ["three"]));
    }

    #[gpui::test]
    fn opens_a_revealed_worker_and_clears_the_reveal_on_collapse(cx: &mut TestAppContext) {
        init(cx);
        let fake = Rc::new(Fake::default());
        *fake.selected.borrow_mut() = Some("one".into());
        let tasks = ["one", "two"]
            .iter()
            .map(|id| summary_task(id, id, false))
            .collect();
        let card = mount(cx, fake.clone(), summary("active", tasks), false);
        card.read_with(cx, |card, cx| assert_eq!(card.open_ids(cx), ["one"]));
        card.update(cx, |card, cx| card.toggle("one", cx));
        assert_eq!(*fake.inspected.borrow(), [None]);
    }

    #[gpui::test]
    fn explains_paused_recovery_and_opens_the_conversation_blocking_resume(
        cx: &mut TestAppContext,
    ) {
        init(cx);
        let fake = Rc::new(Fake::default());
        fake.runs.borrow_mut().push(OrchestrationRunView {
            lead_id: "lead".into(),
            proposal_id: None,
            status: OrchestrationRunStatus::Paused,
            allowed_harnesses: vec![HarnessId::Codex],
            max_workers: 2,
            error: None,
            tasks: vec![OrchestrationTaskView {
                id: "task".into(),
                session_id: "worker".into(),
                title: "Interrupted worker".into(),
                harness: HarnessId::Codex,
                status: OrchestrationTaskStatus::Cancelled,
                error: None,
            }],
        });
        *fake.blocker.borrow_mut() = Some(ResumeBlocker {
            id: "investigation".into(),
            title: "Investigating the failure".into(),
        });
        let mut task = summary_task("worker", "Interrupted worker", false);
        task.status = "cancelled".into();
        let card = mount(cx, fake.clone(), summary("paused", vec![task]), true);
        card.read_with(cx, |card, cx| {
            let panel = card.paused_panel(cx).expect("paused panel");
            assert!(
                panel.message.starts_with(
                    "Resume continues interrupted workers from their retained checkouts"
                )
            );
            assert_eq!(
                panel.blocker.as_ref().map(|blocker| blocker.title.as_str()),
                Some("Investigating the failure")
            );
            assert!(panel.resume_disabled);
            assert!(panel.can_open_blocker);
        });
        card.update(cx, |card, cx| card.open_blocker(cx));
        assert_eq!(*fake.opened.borrow(), ["investigation"]);
    }
}
