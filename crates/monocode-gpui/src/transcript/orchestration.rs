//! An Orchestrator plan in the lead's thread. The card the plan arrives as is
//! where it is reviewed, started, followed and reported, so the workers read
//! like the lead's own subagents instead of living only in the sidebar.
use super::*;
use gpui::AnyElement;
use monocode_core::orchestration::{Plan, State, Team};

const FUCHSIA_300: u32 = 0xf4a8ff;
const AMBER_400: u32 = 0xffb900;
fn tint(color: u32, alpha: f32) -> gpui::Hsla {
    gpui::Hsla::from(gpui::rgb(color)).opacity(alpha)
}

/// Where a plan's card is in its life.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Phase {
    /// Waiting for the user. `blocked` says why it cannot start yet.
    Review {
        blocked: Option<String>,
    },
    Running,
    /// Restored after a restart; nothing runs until the user resumes.
    Paused,
    /// The user stopped it before every assignment finished.
    Stopped,
    /// Every assignment settled. `sending` explains a report not yet sent.
    Finished {
        reported: bool,
        sending: Option<String>,
    },
    /// A newer plan or run replaced it.
    Inactive,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TaskView {
    pub state: State,
    pub needs_input: bool,
    pub session: Option<i64>,
    pub harness: String,
    pub model: String,
    pub steps: usize,
    /// Why it waits, or what went wrong.
    pub note: Option<String>,
    pub outside_scope: Vec<String>,
    /// The end of the worker's report.
    pub report: String,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RunView {
    pub plan: Plan,
    pub phase: Phase,
    pub tasks: Vec<TaskView>,
    /// The user's team choice; none runs every worker on the lead's model.
    pub team: Option<Team>,
}
impl RunView {
    /// A plan nobody has started, as the planning turn left it.
    pub(crate) fn review(
        plan: Plan,
        phase: Phase,
        harness: &str,
        model: &str,
        team: Option<Team>,
    ) -> Self {
        let tasks = plan
            .tasks
            .iter()
            .map(|task| TaskView {
                state: State::Pending,
                needs_input: false,
                session: None,
                harness: task.provider.clone().unwrap_or_else(|| harness.into()),
                model: monocode_core::orchestration::tier_model(team, harness, task, 0)
                    .unwrap_or_else(|| model.into()),
                steps: 0,
                note: (!task.depends_on.is_empty())
                    .then(|| format!("after {}", task.depends_on.join(", "))),
                outside_scope: Vec::new(),
                report: String::new(),
            })
            .collect();
        Self {
            plan,
            phase,
            tasks,
            team,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Action {
    Start,
    Dismiss,
    /// Drops an assignment from the plan under review.
    Remove(String),
    /// Picks the team behind the plan under review; none is the lead's model.
    Team(Option<Team>),
    Stop,
    Resume,
    Retry(usize),
    SendReport,
}
/// A button pressed on the card of the plan submitted by `call`.
#[derive(Clone, Debug)]
pub(crate) struct OrchestrationAction {
    pub call: String,
    pub action: Action,
}
impl gpui::EventEmitter<OrchestrationAction> for TranscriptView {}

impl TranscriptView {
    /// Cards keyed by the tool call that submitted their plan.
    pub(crate) fn set_orchestration(
        &mut self,
        runs: HashMap<String, RunView>,
        cx: &mut Context<Self>,
    ) {
        if self.orchestration == runs {
            return;
        }
        self.orchestration = runs;
        self.remeasure_orchestration();
        cx.notify();
    }
    fn remeasure_orchestration(&mut self) {
        for (ix, row) in self.rows.iter().enumerate() {
            if matches!(&row.body, Body::Tool { call, .. } if call.orchestration_plan().is_some()) {
                self.list.remeasure_items(ix..ix + 1);
            }
        }
    }
    pub(crate) fn toggle_orchestration_task(
        &mut self,
        call: &str,
        ix: usize,
        cx: &mut Context<Self>,
    ) {
        let key = (call.to_owned(), ix);
        if !self.orchestration_open.remove(&key) {
            self.orchestration_open.insert(key);
        }
        self.list.pause_following_tail();
        self.remeasure_orchestration();
        cx.notify();
    }
}

fn tone(task: &TaskView) -> (gpui::Hsla, &'static str) {
    let (color, label) = if task.needs_input {
        (0xfbbf24, "Needs input")
    } else {
        match task.state {
            State::Pending => (0x94a3b8, "Queued"),
            State::Starting => (0x7dd3fc, "Starting"),
            State::Running => (0x7dd3fc, "Working"),
            State::Done => (0x6ee7b7, "Done"),
            State::Failed => (0xf87171, "Failed"),
            State::Stopped => (0x94a3b8, "Stopped"),
            State::Blocked => (0xfbbf24, "Blocked"),
            State::Interrupted => (0xfbbf24, "Interrupted"),
        }
    };
    (gpui::rgb(color).into(), label)
}

fn button(
    id: impl Into<gpui::ElementId>,
    label: &'static str,
    primary: bool,
    disabled: bool,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .role(gpui::Role::Button)
        .aria_label(label)
        .px_2()
        .py_0p5()
        .rounded(px(6.))
        .text_size(px(12.))
        .when(primary, |d| d.bg(content(0.12)).text_color(content(0.9)))
        .when(!primary, |d| d.text_color(content(0.6)))
        .when(disabled, |d| d.opacity(0.45))
        .when(!disabled, |d| {
            d.cursor_pointer()
                .hover(|s| s.bg(content(0.16)).text_color(content(0.95)))
        })
        .child(label)
}

/// One button on the card: element id, label, primary, disabled, action.
type CardButton = (&'static str, &'static str, bool, bool, Action);
type CardHandler = Box<dyn Fn(&mut TranscriptView, &mut Context<TranscriptView>)>;
/// The card's buttons, and why the main one is unavailable.
pub(crate) fn actions(phase: &Phase) -> (Vec<CardButton>, Option<String>) {
    match phase {
        Phase::Review { blocked } => (
            vec![
                (
                    "orch-start",
                    "Start agents",
                    true,
                    blocked.is_some(),
                    Action::Start,
                ),
                ("orch-dismiss", "Dismiss", false, false, Action::Dismiss),
            ],
            blocked.clone(),
        ),
        Phase::Running => (
            vec![("orch-stop", "Stop agents", false, false, Action::Stop)],
            None,
        ),
        Phase::Paused => (
            vec![
                ("orch-resume", "Resume", true, false, Action::Resume),
                ("orch-stop", "Stop", false, false, Action::Stop),
            ],
            None,
        ),
        Phase::Stopped => (
            vec![
                ("orch-resume", "Resume", true, false, Action::Resume),
                (
                    "orch-report",
                    "Send report",
                    false,
                    false,
                    Action::SendReport,
                ),
            ],
            None,
        ),
        Phase::Finished {
            reported: false,
            sending,
        } => (
            vec![(
                "orch-report",
                "Send report",
                true,
                sending.is_some(),
                Action::SendReport,
            )],
            sending.clone(),
        ),
        Phase::Finished { .. } | Phase::Inactive => (Vec::new(), None),
    }
}

pub(crate) fn tone_of(task: &TaskView) -> (gpui::Hsla, &'static str) {
    tone(task)
}

pub(crate) fn headline(run: &RunView) -> String {
    let count = |f: &dyn Fn(&TaskView) -> bool| run.tasks.iter().filter(|t| f(t)).count();
    let total = run.tasks.len();
    let done = count(&|t| t.state == State::Done);
    let working = count(&|t| matches!(t.state, State::Starting | State::Running) && !t.needs_input);
    let input = count(&|t| t.needs_input);
    let failed = count(&|t| t.state == State::Failed);
    let stopped = count(&|t| t.state == State::Stopped);
    let interrupted = count(&|t| t.state == State::Interrupted);
    let blocked = count(&|t| t.state == State::Blocked);
    let mut parts = vec![match &run.phase {
        Phase::Review { .. } => format!(
            "{total} assignment{} to review",
            if total == 1 { "" } else { "s" }
        ),
        Phase::Inactive => format!("{total} assignments"),
        _ => format!("{done} of {total} done"),
    }];
    if !matches!(run.phase, Phase::Review { .. } | Phase::Inactive) {
        if working > 0 {
            parts.push(format!("{working} working"));
        }
        if input > 0 {
            parts.push(format!(
                "{input} need{} input",
                if input == 1 { "s" } else { "" }
            ));
        }
        for (n, label) in [
            (failed, "failed"),
            (stopped, "stopped"),
            (interrupted, "interrupted"),
            (blocked, "blocked"),
        ] {
            if n > 0 {
                parts.push(format!("{n} {label}"));
            }
        }
    }
    parts.push(
        match &run.phase {
            Phase::Review { .. } => "not started",
            Phase::Running => "running",
            Phase::Paused => "paused after restart",
            Phase::Stopped => "stopped",
            Phase::Finished { reported: true, .. } => "reported",
            Phase::Finished { .. } => "finished",
            Phase::Inactive => "replaced",
        }
        .into(),
    );
    parts.join(" · ")
}

pub(super) fn render(
    view: &mut TranscriptView,
    call: &ToolCall,
    plan: &Plan,
    start: usize,
    cx: &mut Context<TranscriptView>,
) -> AnyElement {
    let run = view
        .orchestration
        .get(&call.id)
        .cloned()
        .unwrap_or_else(|| {
            RunView::review(plan.clone(), Phase::Inactive, &view.provider, "", None)
        });
    let key = call.id.clone();
    let action = move |action: Action| -> CardHandler {
        let call = key.clone();
        Box::new(
            move |_: &mut TranscriptView, cx: &mut Context<TranscriptView>| {
                cx.emit(OrchestrationAction {
                    call: call.clone(),
                    action: action.clone(),
                })
            },
        )
    };
    let (actions, reason) = actions(&run.phase);
    let buttons: Vec<AnyElement> = actions
        .into_iter()
        .map(|(id, label, primary, disabled, act)| {
            let handler = action(act);
            button((id, start), label, primary, disabled)
                .when(!disabled, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| handler(this, cx)))
                })
                .into_any_element()
        })
        .collect();
    let reviewing = matches!(run.phase, Phase::Review { .. });
    let live = !matches!(run.phase, Phase::Inactive);
    let accent = tint(FUCHSIA_300, 0.8);
    let rows = run
        .plan
        .tasks
        .iter()
        .zip(&run.tasks)
        .enumerate()
        .map(|(ix, (task, state))| {
            let (color, label) = tone(state);
            let open = view.orchestration_open.contains(&(call.id.clone(), ix));
            let group = SharedString::from(format!("orch-task-{start}-{ix}"));
            let retry = live
                && !reviewing
                && matches!(
                    state.state,
                    State::Failed | State::Stopped | State::Interrupted
                );
            let removable = reviewing && run.plan.tasks.len() > 1;
            let session = state.session;
            let id = task.id.clone();
            let toggle_call = call.id.clone();
            let mut detail: Vec<(&'static str, String)> = vec![("Scope", task.files.join(", "))];
            if !task.depends_on.is_empty() {
                detail.push(("After", task.depends_on.join(", ")));
            }
            if open {
                detail.push((
                    "Assignment",
                    monocode_core::response::limit_middle(&task.prompt, 2_400),
                ));
                if !state.report.trim().is_empty() {
                    detail.push(("Report", state.report.clone()));
                }
            }
            if !state.outside_scope.is_empty() {
                detail.push(("Outside scope", state.outside_scope.join(", ")));
            }
            let retry_handler = action(Action::Retry(ix));
            let remove_handler = action(Action::Remove(id));
            div()
                .flex()
                .flex_col()
                .border_t_1()
                .border_color(content(0.06))
                .child(
                    div()
                        .group(group.clone())
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_2()
                        .py_1p5()
                        .child(
                            div()
                                .id(("orch-task", start * 16 + ix))
                                .role(gpui::Role::Button)
                                .aria_label(format!(
                                    "{} details for {}",
                                    if open { "Hide" } else { "Show" },
                                    task.title
                                ))
                                .aria_expanded(open)
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .items_center()
                                .gap_2()
                                .cursor_pointer()
                                .child(
                                    div()
                                        .size(px(7.))
                                        .flex_none()
                                        .rounded_full()
                                        .bg(if reviewing { content(0.3) } else { color }),
                                )
                                .child(provider_icon(&state.harness, 14.))
                                .child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(px(13.))
                                        .text_color(content(0.85))
                                        .child(task.title.clone()),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(px(11.))
                                        .text_color(content(0.4))
                                        .child(
                                            [
                                                Some(state.model.clone()).filter(|m| !m.is_empty()),
                                                (state.steps > 0).then(|| {
                                                    format!(
                                                        "{} step{}",
                                                        state.steps,
                                                        if state.steps == 1 { "" } else { "s" }
                                                    )
                                                }),
                                                state.note.clone(),
                                            ]
                                            .into_iter()
                                            .flatten()
                                            .collect::<Vec<_>>()
                                            .join(" · "),
                                        ),
                                )
                                .when(!reviewing, |d| {
                                    d.child(
                                        div()
                                            .px_1p5()
                                            .rounded_full()
                                            .border_1()
                                            .border_color(color.opacity(0.25))
                                            .bg(color.opacity(0.1))
                                            .text_size(px(11.))
                                            .text_color(color)
                                            .child(label),
                                    )
                                })
                                .child(
                                    icon(
                                        if open {
                                            "chevron-down"
                                        } else {
                                            "chevron-right"
                                        },
                                        12.,
                                    )
                                    .text_color(content(0.4)),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.toggle_orchestration_task(&toggle_call, ix, cx)
                                })),
                        )
                        .child(
                            div()
                                .flex_none()
                                .w(px(84.))
                                .flex()
                                .items_center()
                                .justify_end()
                                .gap_0p5()
                                .when(retry, |d| {
                                    d.child(
                                        button(
                                            ("orch-retry", start * 16 + ix),
                                            "Retry",
                                            false,
                                            false,
                                        )
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                retry_handler(this, cx)
                                            }),
                                        ),
                                    )
                                })
                                .when(removable, |d| {
                                    d.child(
                                        div()
                                            .id(("orch-remove", start * 16 + ix))
                                            .role(gpui::Role::Button)
                                            .aria_label(format!("Remove {}", task.title))
                                            .p_1()
                                            .rounded(px(4.))
                                            .cursor_pointer()
                                            .opacity(0.)
                                            .group_hover(group.clone(), |s| s.opacity(1.))
                                            .hover(|s| s.bg(content(0.08)))
                                            .child(icon("close", 12.).text_color(content(0.55)))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                remove_handler(this, cx)
                                            })),
                                    )
                                })
                                .when_some(session, |d, session| {
                                    d.child(
                                        div()
                                            .id(("orch-open", start * 16 + ix))
                                            .role(gpui::Role::Button)
                                            .aria_label(format!("Open {}", task.title))
                                            .p_1()
                                            .rounded(px(4.))
                                            .cursor_pointer()
                                            .hover(|s| s.bg(content(0.08)))
                                            .child(
                                                icon("external-link", 14.).text_color(content(0.5)),
                                            )
                                            .on_click(cx.listener(move |_, _, _, cx| {
                                                cx.emit(crate::conversation::OpenThread(session))
                                            })),
                                    )
                                }),
                        ),
                )
                .when(open || !state.outside_scope.is_empty(), |d| {
                    d.child(
                        div()
                            .pl(px(30.))
                            .pr_2()
                            .pb_2()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .children(detail.into_iter().map(|(name, text)| {
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(18.))
                                    .child(
                                        div()
                                            .text_color(if name == "Outside scope" {
                                                tint(AMBER_400, 0.9)
                                            } else {
                                                content(0.45)
                                            })
                                            .child(name),
                                    )
                                    .child(div().text_color(content(0.75)).child(text))
                            })),
                    )
                })
        });
    div()
        .id(("orchestration", start))
        .role(gpui::Role::Group)
        .aria_label(format!("Orchestration plan: {}", run.plan.title))
        .w_full()
        .min_w_0()
        .my_1()
        .rounded(px(10.))
        .border_1()
        .border_color(tint(FUCHSIA_300, if live { 0.25 } else { 0.12 }))
        .bg(tint(FUCHSIA_300, if live { 0.04 } else { 0.015 }))
        .flex()
        .flex_col()
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .px_2p5()
                .py_2()
                .child(icon("share", 16.).text_color(accent))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(px(14.))
                        .text_color(content(0.85))
                        .child(run.plan.title.clone()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(11.))
                        .text_color(content(0.45))
                        .child(headline(&run)),
                )
                .children(buttons),
        )
        .when(!run.plan.summary.trim().is_empty(), |d| {
            d.child(
                div()
                    .px_2p5()
                    .pb_2()
                    .text_size(px(12.))
                    .line_height(px(18.))
                    .text_color(content(0.6))
                    .child(monocode_core::response::limit(&run.plan.summary, 600)),
            )
        })
        .when_some(reason, |d, reason| {
            d.child(
                div()
                    .px_2p5()
                    .pb_2()
                    .text_size(px(11.))
                    .text_color(tint(AMBER_400, 0.9))
                    .child(reason),
            )
        })
        .children(rows)
        .when(reviewing, |d| d.child(team_row(&run, start, &action, cx)))
        .when(reviewing, |d| {
            d.child(
                div()
                    .px_2p5()
                    .py_1p5()
                    .border_t_1()
                    .border_color(content(0.06))
                    .text_size(px(11.))
                    .text_color(content(0.4))
                    .child("Reply to revise the plan. Workers share this checkout; overlapping scopes run one after another."),
            )
        })
        .into_any_element()
}

/// The review card's team chooser: the lead's model for everything, or a
/// tier that picks each Claude worker's model from its difficulty.
fn team_row(
    run: &RunView,
    start: usize,
    action: &impl Fn(Action) -> Box<dyn Fn(&mut TranscriptView, &mut Context<TranscriptView>)>,
    cx: &mut Context<TranscriptView>,
) -> AnyElement {
    let choices: [(&'static str, Option<Team>); 4] = [
        ("Lead's model", None),
        ("Cheap", Some(Team::Cheap)),
        ("Balanced", Some(Team::Balanced)),
        ("Strong", Some(Team::Strong)),
    ];
    let hint = run
        .team
        .map_or("Every worker runs on this chat's model.", Team::hint);
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_1p5()
        .px_2p5()
        .py_1p5()
        .border_t_1()
        .border_color(content(0.06))
        .child(
            div()
                .text_size(px(11.))
                .text_color(content(0.5))
                .child("Team"),
        )
        .children(choices.into_iter().enumerate().map(|(ix, (label, team))| {
            let handler = action(Action::Team(team));
            let selected = run.team == team;
            div()
                .id(("orch-team", start * 8 + ix))
                .role(gpui::Role::RadioButton)
                .aria_label(label)
                .aria_selected(selected)
                .px_2()
                .py_0p5()
                .rounded(px(6.))
                .text_size(px(12.))
                .cursor_pointer()
                .when(selected, |d| {
                    d.bg(tint(FUCHSIA_300, 0.16)).text_color(content(0.92))
                })
                .when(!selected, |d| {
                    d.text_color(content(0.6))
                        .hover(|s| s.bg(content(0.1)).text_color(content(0.9)))
                })
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| handler(this, cx)))
        }))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(11.))
                .text_color(content(0.4))
                .child(hint),
        )
        .into_any_element()
}
