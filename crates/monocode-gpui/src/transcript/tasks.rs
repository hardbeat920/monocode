//! Native task snapshots and delegated runs, driven only by structured metadata.
use super::{
    TranscriptView,
    live_phase::{LiveScroll, live_phase},
};
use crate::theme::*;
use crate::{
    markdown, markdown_model,
    reveal::{Disclosure, Motion},
};
use gpui::{
    AnyElement, Context, Pixels, Role, SharedString, Transformation, div, percentage, prelude::*,
    px, rgb,
};
use monocode_core::tool::{
    ToolCall, ToolState,
    tasks::{AgentRun, AgentStep, Panel, TaskList, TaskStatus},
};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    hash::{Hash, Hasher},
    time::{Duration, Instant},
};

pub(super) fn render(
    view: &mut TranscriptView,
    call: &ToolCall,
    start: usize,
    width: Option<Pixels>,
    cx: &mut Context<TranscriptView>,
) -> Option<AnyElement> {
    Some(match call.panel()? {
        Panel::Tasks(list) => task_list(&list, start),
        Panel::Agent(run) => agent(view, call, &run, start, width, cx),
    })
}
fn task_list(list: &TaskList, start: usize) -> AnyElement {
    div()
        .id(("task-progress", start))
        .role(Role::Group)
        .aria_label(format!("Task progress: {}", list.progress()))
        .w_full()
        .min_w_0()
        .my_1()
        .rounded(px(10.))
        .border_1()
        .border_color(content(0.1))
        .bg(content(0.035))
        .child(
            div()
                .px_2p5()
                .py_2()
                .border_b_1()
                .border_color(content(0.1))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(icon("list-bullet", 16.).text_color(content(0.45)))
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(12.))
                                .text_color(content(0.85))
                                .child("Tasks"),
                        )
                        .child(
                            div()
                                .rounded_full()
                                .px_2()
                                .py_0p5()
                                .bg(content(0.07))
                                .text_size(px(10.))
                                .text_color(content(0.5))
                                .child(list.progress()),
                        ),
                )
                .when(!list.explanation.is_empty(), |d| {
                    d.child(
                        div()
                            .mt_1()
                            .text_size(px(12.))
                            .text_color(content(0.5))
                            .child(list.explanation.clone()),
                    )
                }),
        )
        .child(
            div()
                .id(("task-items", start))
                .max_h(px(320.))
                .overflow_y_scroll()
                .py_1()
                .when(list.items.is_empty(), |d| {
                    d.child(
                        div()
                            .px_2p5()
                            .py_1()
                            .text_size(px(12.))
                            .text_color(content(0.5))
                            .child("No tasks"),
                    )
                })
                .children(
                    list.items
                        .iter()
                        .take(100)
                        .enumerate()
                        .map(|(index, task)| {
                            let (glyph, color) = match task.status {
                                TaskStatus::Completed => ("check", rgb(0x6ee7b7).into()),
                                TaskStatus::InProgress => ("circle-dashed", rgb(0x7dd3fc).into()),
                                TaskStatus::Cancelled => ("minus", content(0.35)),
                                TaskStatus::Pending => ("context-ring", content(0.35)),
                            };
                            div()
                                .id(("task-item", index))
                                .role(Role::Label)
                                .aria_label(format!("{}: {}", task.status.label(), task.text))
                                .flex()
                                .items_start()
                                .gap_2p5()
                                .px_2p5()
                                .py_1p5()
                                .child(
                                    icon(glyph, 14.)
                                        .mt(px(2.))
                                        .text_color(color)
                                        .flex_shrink_0(),
                                )
                                .child(
                                    div()
                                        .min_w_0()
                                        .flex_1()
                                        .text_size(px(13.))
                                        .line_height(px(18.))
                                        .text_color(content(match task.status {
                                            TaskStatus::Completed | TaskStatus::Cancelled => 0.4,
                                            TaskStatus::InProgress => 0.85,
                                            _ => 0.6,
                                        }))
                                        .when(
                                            matches!(
                                                task.status,
                                                TaskStatus::Completed | TaskStatus::Cancelled
                                            ),
                                            |d| d.line_through(),
                                        )
                                        .child(task.text.clone()),
                                )
                        }),
                )
                .when(list.items.len() > 100, |d| {
                    d.child(
                        div()
                            .px_2p5()
                            .py_1()
                            .text_size(px(11.))
                            .text_color(content(0.45))
                            .child(format!("Showing 100 of {} tasks", list.items.len())),
                    )
                }),
        )
        .into_any_element()
}

/// Disclosure overrides belong to the run/step identity, independently of the
/// surrounding work fold. Updating a streamed run never resets the reader's choice.
#[derive(Default)]
pub(super) struct AgentPanels {
    parts: HashMap<String, PanelPart>,
    scrolls: HashMap<String, LiveScroll>,
    groups: HashMap<String, StepGroup>,
    arrivals: HashMap<String, StepArrival>,
    returned_at: Option<Instant>,
}
struct StepGroup {
    seen: HashSet<String>,
    next: Instant,
}
struct StepArrival {
    motion: Motion,
    source: usize,
}
#[derive(Default)]
struct PanelPart {
    open: Option<bool>,
    motion: Option<Motion>,
    source: usize,
}
impl AgentPanels {
    pub(super) fn animating(&self) -> bool {
        !self.arrivals.is_empty() || self.parts.values().any(|part| part.motion.is_some())
    }
    pub(super) fn animate(&mut self) -> Vec<usize> {
        let now = Instant::now();
        let mut changed = Vec::new();
        for part in self.parts.values_mut() {
            if let Some(motion) = &part.motion {
                changed.push(part.source);
                if motion.finished(now) {
                    part.motion = None;
                }
            }
        }
        self.arrivals.retain(|_, arrival| {
            changed.push(arrival.source);
            !arrival.motion.finished(now)
        });
        changed.sort_unstable();
        changed.dedup();
        changed
    }
    pub(super) fn settle(&mut self) {
        self.arrivals.clear();
        for part in self.parts.values_mut() {
            part.motion = None;
        }
        self.returned_at = Some(Instant::now());
        for scroll in self.scrolls.values_mut() {
            scroll.enable(false);
        }
    }
    fn observe_steps(&mut self, key: &str, steps: &[&AgentStep], source: usize, animate: bool) {
        let now = Instant::now();
        let returning = self
            .returned_at
            .is_some_and(|returned| now.duration_since(returned) < Duration::from_millis(400));
        let mounted = self.groups.contains_key(key);
        let group = self
            .groups
            .entry(key.to_owned())
            .or_insert_with(|| StepGroup {
                seen: HashSet::new(),
                next: now,
            });
        for step in steps {
            if !group.seen.insert(step.id.clone()) {
                continue;
            }
            if !mounted || !animate || returning {
                continue;
            }
            let queued = group.next.max(now);
            let wait = queued.duration_since(now).as_secs_f32();
            let pace = (0.48 * ((2. - wait) / (2. - 0.96)).min(1.)).max(0.16);
            let mut motion = Motion::new(true, true, now);
            motion.start = queued;
            motion.duration = Duration::from_secs_f32(pace);
            group.next = queued + motion.duration;
            self.arrivals
                .insert(format!("{key}:{}", step.id), StepArrival { motion, source });
        }
    }
    pub(super) fn part_open(&self, key: &str, default: bool) -> bool {
        self.parts
            .get(key)
            .and_then(|part| part.open)
            .unwrap_or(default)
    }
    fn open(&self, key: &str, default: bool) -> bool {
        self.part_open(key, default)
    }
    fn override_open(&self, key: &str) -> bool {
        self.parts.get(key).and_then(|part| part.open) == Some(true)
    }
    fn visible(&self, key: &str, open: bool) -> bool {
        open || self
            .parts
            .get(key)
            .is_some_and(|part| part.motion.is_some())
    }
    fn progress(&self, key: &str, open: bool) -> (f32, f32, f32) {
        self.parts
            .get(key)
            .and_then(|part| part.motion.as_ref())
            .map_or(if open { (1., 1., 1.) } else { (0., 0., 0.) }, |motion| {
                let now = Instant::now();
                let (height, opacity) = motion.value(now);
                (height, opacity, motion.rotation(now))
            })
    }
}

impl TranscriptView {
    pub(super) fn toggle_agent_part(
        &mut self,
        key: String,
        open: bool,
        start: usize,
        cx: &mut Context<Self>,
    ) {
        let part = self.agent_panels.parts.entry(key).or_default();
        part.open = Some(open);
        part.source = start;
        if self.visible && !cx.reduce_motion() {
            let now = Instant::now();
            part.motion = Some(part.motion.as_ref().map_or_else(
                || Motion::phase(start, open, now),
                |motion| motion.redirect(open, now),
            ));
        } else {
            part.motion = None;
        }
        self.list.pause_following_tail();
        let phase = self
            .rows
            .iter()
            .find(|row| row.key.0 == start && row.in_phase())
            .and_then(|row| row.phase_group());
        for (ix, row) in self.rows.iter().enumerate() {
            if row.key.0 == start
                || matches!(row.body, super::Body::Phase { group, .. } if Some(group) == phase && self.windowed_phase(group))
            {
                self.list.remeasure_items(ix..ix + 1);
            }
        }
        cx.notify();
    }
}

fn panel_width(view: &TranscriptView, start: usize) -> Pixels {
    let inset = view
        .rows
        .iter()
        .find(|row| row.key == (start, 2))
        .map_or(0., |row| {
            (if row.work { 20. } else { 0. }) + if row.in_phase() { 20. } else { 0. }
        });
    px((f32::from(view.viewport.size.width).min(COLUMN) - 32. - inset).max(1.))
}

fn disclosure(
    view: &TranscriptView,
    key: &str,
    open: bool,
    width: Pixels,
    body: AnyElement,
) -> AnyElement {
    let (height, opacity, _) = view.agent_panels.progress(key, open);
    if height >= 1. {
        return body;
    }
    div()
        .w_full()
        .min_w_0()
        .overflow_hidden()
        .opacity(opacity)
        .child(Disclosure::new(body, width, height))
        .into_any_element()
}

fn agent(
    view: &mut TranscriptView,
    call: &ToolCall,
    run: &AgentRun,
    start: usize,
    available_width: Option<Pixels>,
    cx: &mut Context<TranscriptView>,
) -> AnyElement {
    let name = run.display_name();
    let failed = matches!(call.state, ToolState::Failed | ToolState::Cancelled);
    let active = view.streaming
        && call.state == ToolState::Running
        && view
            .source
            .iter()
            .rposition(monocode_core::transcript::Row::is_prompt)
            .is_none_or(|prompt| start > prompt);
    let report = call.agent_report();
    let omitted = call
        .display
        .as_ref()
        .is_some_and(|meta| meta["detailOmitted"] == true);
    let has_detail = !run.steps.is_empty() || report.is_some() || omitted;
    let key = format!("agent:{start}:{}", call.id);
    let open = view.agent_panels.open(&key, failed);
    if !open {
        // Work arriving behind the row is history when it opens again.
        for phase in agent_phases(run) {
            let phase_key = format!("agent-phase:{start}:{}:{}", call.id, phase.id);
            view.agent_panels
                .observe_steps(&phase_key, &phase.steps, start, false);
        }
    }
    let rotation = view.agent_panels.progress(&key, open).2;
    let toggle_key = key.clone();
    let status = run.status_line(&call.state);
    let model = run.model_name(
        monocode_core::provider::Kind::parse(&view.provider)
            .unwrap_or(monocode_core::provider::Kind::Codex),
    );
    let header = div()
        .id(SharedString::from(key.clone()))
        .debug_selector(move || format!("subagent-{start}"))
        .role(if has_detail {
            Role::Button
        } else {
            Role::Label
        })
        .aria_label(if has_detail {
            format!("{} {name}'s work", if open { "Hide" } else { "Show" })
        } else {
            format!("Subagent: {name}")
        })
        .when(has_detail, |d| d.aria_expanded(open))
        .tooltip({
            let brief = run.display_brief();
            move |_, cx| cx.new(|_| AgentBrief(brief.clone())).into()
        })
        .group("subagent-row")
        .flex()
        .items_center()
        .gap_2()
        .w_full()
        .min_w_0()
        .mx(px(-6.))
        .px_1p5()
        .py_1()
        .rounded(px(6.))
        .text_size(px(14.))
        .line_height(px(20.))
        .when(has_detail, |d| {
            d.cursor_pointer().hover(|s| s.bg(content(0.08)))
        })
        .when(has_detail && open, |d| d.bg(content(0.08)))
        .child(super::mascot::subagent_mascot(
            &name,
            &call.state,
            active && view.visible,
        ))
        .child(
            div()
                .flex()
                .items_baseline()
                .gap_2()
                .min_w_0()
                .flex_1()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(if failed {
                            rgb(0xf87171).into()
                        } else {
                            content(0.75)
                        })
                        .group_hover("subagent-row", |s| {
                            s.text_color(if failed {
                                rgb(0xf87171).into()
                            } else {
                                content(1.)
                            })
                        })
                        .child(if active && view.visible && !cx.reduce_motion() {
                            crate::activity::shimmer(
                                name.clone(),
                                view.started_at.elapsed().as_secs_f32(),
                            )
                        } else {
                            div().child(name).into_any_element()
                        }),
                )
                .when(model.is_some() || !status.is_empty(), |d| {
                    d.child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap_2()
                            .min_w_0()
                            .max_w(gpui::relative(0.55))
                            .flex_shrink_0()
                            .text_size(px(12.))
                            .text_color(content(0.4))
                            .when_some(model, |d, model| {
                                d.child(div().min_w_0().truncate().child(model))
                            })
                            .when(!status.is_empty(), |d| {
                                d.child(div().flex_shrink_0().child(status))
                            }),
                    )
                }),
        )
        .child(if has_detail {
            icon("chevron-right", 14.)
                .flex_shrink_0()
                .text_color(content(0.35))
                .with_transformation(Transformation::rotate(percentage(rotation * 0.25)))
                .into_any_element()
        } else {
            div().size(px(14.)).flex_shrink_0().into_any_element()
        })
        .on_click(cx.listener(move |this, _, _, cx| {
            if has_detail {
                this.toggle_agent_part(toggle_key.clone(), !open, start, cx);
            }
        }));
    let mut panel = div().w_full().min_w_0().flex().flex_col().child(header);
    if has_detail && view.agent_panels.visible(&key, open) {
        let width = available_width.unwrap_or_else(|| panel_width(view, start));
        let trail = agent_trail(view, call, run, start, active, width, cx);
        let mut body = div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .pb_1()
            .child(trail);
        if let Some(report) = report {
            let report = if failed {
                div()
                    .font_family(crate::fonts::mono())
                    .text_size(px(12.))
                    .line_height(px(20.))
                    .text_color({
                        let mut color: gpui::Hsla = rgb(0xf87171).into();
                        color.a = 0.8;
                        color
                    })
                    .child(report.to_owned())
                    .into_any_element()
            } else {
                markdown_text(
                    view,
                    report,
                    &format!("{}:report", call.id),
                    markdown::Tone::Normal,
                    cx,
                )
            };
            body = body.child(
                div()
                    .relative()
                    .pl(px(20.))
                    .py_1()
                    .child(crate::activity::rail(true))
                    .child(report),
            );
        }
        panel = panel.child(disclosure(view, &key, open, width, body.into_any_element()));
    }
    panel.into_any_element()
}

/// Direct port of buildActivityPhases: prose titles the work that follows it;
/// private reasoning remains a step, and mixed tool kinds stay in one phase.
struct AgentPhase<'a> {
    id: &'a str,
    headline: Option<&'a AgentStep>,
    steps: Vec<&'a AgentStep>,
    kind: &'static str,
    counts: [usize; 5],
}
/// The phase kind a tool step counts toward, as an index into the kinds
/// `agent_phases` names. Classifying builds the step's full tool call and runs
/// its presentation, and agent rows group their steps on every frame, so a
/// finished step, which no longer changes, keeps its answer.
fn step_kind(run: &AgentRun, step: &AgentStep) -> usize {
    thread_local! {
        static SETTLED: RefCell<HashMap<u64, usize>> = RefCell::default();
    }
    let settled = matches!(
        step.state,
        Some(ToolState::Completed | ToolState::Failed | ToolState::Cancelled)
    );
    let key = {
        let mut hasher = std::hash::DefaultHasher::new();
        (&run.ids, &step.id, &step.text).hash(&mut hasher);
        hasher.finish()
    };
    if settled && let Some(kind) = SETTLED.with_borrow(|cache| cache.get(&key).copied()) {
        return kind;
    }
    let kind = match step.to_tool_call().summary().0.as_str() {
        "Edit" | "Write" => 0,
        "Run" => 1,
        "Agent" => 2,
        "Read" | "List" | "Search" | "Find" => 3,
        _ => 4,
    };
    if settled {
        SETTLED.with_borrow_mut(|cache| {
            if cache.len() >= 4096 {
                cache.clear();
            }
            cache.insert(key, kind);
        });
    }
    kind
}
fn agent_phases(run: &AgentRun) -> Vec<AgentPhase<'_>> {
    let mut phases: Vec<AgentPhase<'_>> = Vec::new();
    for step in &run.steps {
        if step.kind != "tool" && step.text.trim().is_empty() {
            continue;
        }
        let prose = step.kind != "tool" && step.kind != "reasoning";
        let narrating = phases
            .last()
            .is_some_and(|phase| matches!(phase.kind, "think" | "note"));
        if phases.is_empty() || (prose && !narrating) {
            phases.push(AgentPhase {
                id: &step.id,
                headline: None,
                steps: Vec::new(),
                kind: if prose { "note" } else { "think" },
                counts: [0; 5],
            });
        }
        let current = phases.last_mut().unwrap();
        if prose && current.headline.is_none() {
            current.headline = Some(step);
            current.kind = "note";
            continue;
        }
        current.steps.push(step);
        if step.kind == "tool" {
            current.counts[step_kind(run, step)] += 1;
            let dominant = (0..5)
                .max_by_key(|&kind| (current.counts[kind], 5 - kind))
                .unwrap();
            current.kind = ["edit", "run", "agent", "research", "other"][dominant];
        }
    }
    phases
}
fn phase_icon(kind: &str) -> &'static str {
    match kind {
        "edit" => "pencil-line",
        "run" => "terminal",
        "agent" => "bot",
        "research" => "search",
        "other" => "wrench",
        _ => "minus",
    }
}
pub(crate) fn prose_summary(text: &str) -> String {
    markdown_model::parse_full(text)
        .blocks
        .iter()
        .flat_map(|block| markdown::rows(&block.block))
        .map(|row| markdown::row_text(&row))
        .find(|text| !text.trim().is_empty())
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn agent_trail(
    view: &mut TranscriptView,
    parent: &ToolCall,
    run: &AgentRun,
    start: usize,
    active: bool,
    width: Pixels,
    cx: &mut Context<TranscriptView>,
) -> AnyElement {
    let phases = agent_phases(run);
    let count = phases.len();
    let mut trail = div().w_full().min_w_0().flex().flex_col().gap_1();
    for (index, phase) in phases.into_iter().enumerate() {
        let live = active && index + 1 == count;
        let key = format!("agent-phase:{start}:{}:{}", parent.id, phase.id);
        let open = view.agent_panels.open(&key, live);
        if let Some(scroll) = view.agent_panels.scrolls.get_mut(&key) {
            scroll.enable(live && open && view.visible);
        }
        view.agent_panels.observe_steps(
            &key,
            &phase.steps,
            start,
            live && open && view.visible && !cx.reduce_motion(),
        );
        let calls: Vec<_> = phase
            .steps
            .iter()
            .filter(|step| step.kind == "tool")
            .map(|step| step.to_tool_call())
            .collect();
        let failed_agents = calls
            .iter()
            .filter(|call| {
                matches!(call.state, ToolState::Failed | ToolState::Cancelled)
                    && matches!(call.panel(), Some(Panel::Agent(_)))
            })
            .count();
        let title = if failed_agents > 0 {
            if failed_agents == 1 {
                "Subagent failed".to_owned()
            } else {
                format!("{failed_agents} subagents failed")
            }
        } else {
            phase
                .headline
                .map(|step| prose_summary(&step.text))
                .filter(|text| !text.is_empty())
                .unwrap_or_else(|| {
                    if calls.is_empty() {
                        if live { "Thinking" } else { "Thought" }.to_owned()
                    } else {
                        crate::activity::summary(&calls, live).0
                    }
                })
        };
        let glyph = phase_icon(phase.kind);
        if phase.headline.is_none() && phase.steps.len() == 1 {
            let inset = if phase.kind == "think" { 0. } else { 20. };
            let row = agent_step(
                view,
                parent,
                phase.steps[0],
                start,
                true,
                (width - px(inset)).max(px(1.)),
                cx,
            );
            trail = trail.child(
                div()
                    .flex()
                    .items_start()
                    .gap_1p5()
                    .when(phase.kind != "think", |d| {
                        d.child(icon(glyph, 14.).mt(px(7.)).text_color(content(0.45)))
                    })
                    .child(div().flex_1().min_w_0().child(row)),
            );
            continue;
        }
        let has_headline_detail = phase
            .headline
            .is_some_and(|step| step.text.trim().lines().any(|line| line.trim().is_empty()));
        let inert = phase.steps.is_empty() && !has_headline_detail;
        let toggle_key = key.clone();
        let rotation = view.agent_panels.progress(&key, open).2;
        let mut group = div()
            .id(SharedString::from(key.clone()))
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("header")
                    .role(if inert { Role::Label } else { Role::Button })
                    .aria_label(format!(
                        "{} the steps for {title}",
                        if open { "Hide" } else { "Show" }
                    ))
                    .when(!inert, |d| d.aria_expanded(open).cursor_pointer())
                    .group("agent-phase")
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .py_1()
                    .text_size(px(14.))
                    .line_height(px(20.))
                    .text_color(content(0.5))
                    .child(
                        div()
                            .relative()
                            .size(px(14.))
                            .flex_shrink_0()
                            .when(phase.kind != "think", |d| {
                                d.child(
                                    icon(glyph, 14.).group_hover("agent-phase", |s| s.opacity(0.)),
                                )
                            })
                            .when(!inert, |d| {
                                d.child(
                                    icon("chevron-right", 14.)
                                        .absolute()
                                        .top_0()
                                        .left_0()
                                        .opacity(0.)
                                        .with_transformation(Transformation::rotate(percentage(
                                            rotation * 0.25,
                                        )))
                                        .group_hover("agent-phase", |s| s.opacity(1.)),
                                )
                            }),
                    )
                    .child(div().flex_1().min_w_0().truncate().child(
                        if live && view.visible && !cx.reduce_motion() {
                            crate::activity::shimmer(title, view.started_at.elapsed().as_secs_f32())
                        } else {
                            div().child(title).into_any_element()
                        },
                    ))
                    .hover(|s| s.text_color(content(0.8)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !inert {
                            this.toggle_agent_part(toggle_key.clone(), !open, start, cx);
                        }
                    })),
            );
        if !inert && view.agent_panels.visible(&key, open) {
            let show_headline = has_headline_detail && view.agent_panels.override_open(&key);
            let mut steps = div().w_full().min_w_0().flex().flex_col();
            if show_headline {
                let headline = phase.headline.unwrap();
                let body = markdown_text(
                    view,
                    &headline.text,
                    &format!("{}:{}:headline", parent.id, phase.id),
                    markdown::Tone::Normal,
                    cx,
                );
                steps = steps.child(
                    div()
                        .relative()
                        .pl(px(20.))
                        .py_1()
                        .child(crate::activity::rail(phase.steps.is_empty()))
                        .child(body),
                );
            }
            for (index, step) in phase.steps.iter().enumerate() {
                let row = agent_step(
                    view,
                    parent,
                    step,
                    start,
                    true,
                    (width - px(20.)).max(px(1.)),
                    cx,
                );
                let arrival = view
                    .agent_panels
                    .arrivals
                    .get(&format!("{key}:{}", step.id))
                    .map(|arrival| {
                        let now = Instant::now();
                        if now < arrival.motion.start {
                            (0., 0.)
                        } else {
                            arrival.motion.value(now)
                        }
                    });
                let step_body = div()
                    .relative()
                    .pl(px(20.))
                    .child(crate::activity::rail(index + 1 == phase.steps.len()))
                    .child(
                        div()
                            .opacity(arrival.map_or(1., |(_, opacity)| opacity))
                            .child(row),
                    );
                let step_body = if let Some((height, _)) = arrival {
                    div()
                        .w_full()
                        .min_w_0()
                        .overflow_hidden()
                        .child(Disclosure::new(step_body, width, height))
                        .into_any_element()
                } else {
                    step_body.into_any_element()
                };
                steps = steps.child(step_body);
            }
            let body = if live || !open {
                live_phase(
                    steps.into_any_element(),
                    view.agent_panels.scrolls.entry(key.clone()).or_default(),
                    view.phase_max_height,
                    live && open && view.visible,
                    cx,
                )
            } else {
                if let Some(scroll) = view.agent_panels.scrolls.get_mut(&key) {
                    scroll.enable(false);
                }
                steps.into_any_element()
            };
            group = group.child(disclosure(view, &key, open, width, body));
        }
        trail = trail.child(group);
    }
    trail.into_any_element()
}

fn agent_step(
    view: &mut TranscriptView,
    parent: &ToolCall,
    step: &AgentStep,
    start: usize,
    bare: bool,
    width: Pixels,
    cx: &mut Context<TranscriptView>,
) -> AnyElement {
    let identity = format!("{}:{}", parent.id, step.id);
    if step.kind == "tool" {
        let mut call = step.to_tool_call();
        call.id = identity.clone();
        return view.tool_in(call, start, Some(&identity), Some(width), cx);
    }
    let reasoning = step.kind == "reasoning";
    let key = format!("agent-note:{start}:{identity}");
    let open = view.agent_panels.open(&key, false);
    let toggle_key = key.clone();
    let summary = prose_summary(&step.text);
    let mut row = div()
        .id(SharedString::from(key))
        .w_full()
        .min_w_0()
        .flex()
        .flex_col()
        .child(
            div()
                .id("toggle")
                .role(Role::Button)
                .aria_expanded(open)
                .aria_label(format!(
                    "{} {}: {summary}",
                    if open { "Hide" } else { "Show" },
                    if reasoning { "thinking" } else { "note" }
                ))
                .flex()
                .items_center()
                .gap_1p5()
                .py_1()
                .text_size(px(14.))
                .line_height(px(20.))
                .text_color(content(if reasoning { 0.5 } else { 0.7 }))
                .cursor_pointer()
                .hover(|s| s.text_color(content(0.8)))
                .when(!bare, |d| d.child(icon("minus", 14.)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(if open && reasoning {
                            "Thinking".to_owned()
                        } else {
                            summary
                        }),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.toggle_agent_part(toggle_key.clone(), !open, start, cx)
                })),
        );
    if open {
        row = row.child(div().min_w_0().pb_2().when(!bare, |d| d.pl(px(20.))).child(
            markdown_text(
                view,
                &step.text,
                &identity,
                if reasoning {
                    markdown::Tone::Reasoning
                } else {
                    markdown::Tone::Normal
                },
                cx,
            ),
        ));
    }
    row.into_any_element()
}

fn markdown_text(
    view: &mut TranscriptView,
    text: &str,
    identity: &str,
    tone: markdown::Tone,
    cx: &mut Context<TranscriptView>,
) -> AnyElement {
    let tree = markdown_model::parse_full(text);
    let mut body = div().w_full().min_w_0().flex().flex_col();
    for (index, mut row) in tree
        .blocks
        .iter()
        .flat_map(|block| markdown::rows(&block.block))
        .enumerate()
    {
        if let Some(base) = &view.file_link_base {
            markdown::resolve_file_links(&mut row, base);
        }
        let rendered = if let markdown::Part::Code { fence, lines } = &row.part {
            let ix = fence.key as usize;
            let selection = view.text_selection(
                view.rows[ix].key,
                view.messages[view.rows[ix].message].start,
                view.messages[view.rows[ix].message].role == "assistant",
                cx,
            );
            view.code(fence.clone(), lines.clone(), ix, selection, cx)
        } else {
            markdown::body(
                &row,
                format!("agent-md:{identity}:{index}").into(),
                tone,
                None,
                Instant::now(),
            )
        };
        body = body.child(
            div()
                .w_full()
                .min_w_0()
                .flex()
                .pt(px(if index == 0 { 0. } else { row.gap }))
                .pl(px(row.indent.saturating_sub(1) as f32 * 24.))
                .when(row.quote > 0, |d| {
                    d.ml_1().pl_4().border_l_4().border_color(content(0.2))
                })
                .when(row.indent > 0, |d| {
                    d.child(
                        div()
                            .w(px(24.))
                            .flex_shrink_0()
                            .text_color(content(0.78))
                            .child(row.marker.clone().unwrap_or_default()),
                    )
                })
                .child(rendered),
        );
    }
    body.into_any_element()
}

struct AgentBrief(String);
impl gpui::Render for AgentBrief {
    fn render(&mut self, _: &mut gpui::Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .max_w(px(480.))
            .px_2()
            .py_1p5()
            .rounded(px(6.))
            .bg(rgb(0x292929))
            .border_1()
            .border_color(content(0.12))
            .text_size(px(12.))
            .line_height(px(18.))
            .text_color(content(0.85))
            .child(self.0.clone())
    }
}
