//! LiveAgentsPreview.tsx: the sidebar's card of working and unseen finished
//! sessions, across every project.
use super::*;

const LIVE_AGENT_MIN: usize = 2;
const LIVE_AGENT_CAP: usize = 4;
// Tailwind amber-400 and emerald-400.
const AMBER: u32 = 0xfbbf24;
const EMERALD: u32 = 0x34d399;

struct LiveAgent {
    id: i64,
    cwd: String,
    provider: String,
    title: String,
    activity: String,
    started_at: Option<u64>,
    duration_ms: Option<u64>,
    needs_approval: bool,
    done: bool,
}

/// liveAgents.ts's formatLiveElapsed.
fn live_elapsed(ms: u64) -> String {
    let seconds = ((ms + 500) / 1000).max(1);
    if seconds < 60 {
        return format!("{seconds}s");
    }
    let (minutes, rest) = (seconds / 60, seconds % 60);
    if minutes < 60 {
        return if rest > 0 {
            format!("{minutes}m {rest}s")
        } else {
            format!("{minutes}m")
        };
    }
    let (hours, rest) = (minutes / 60, minutes % 60);
    if rest > 0 {
        format!("{hours}h {rest}m")
    } else {
        format!("{hours}h")
    }
}

impl Shell {
    fn live_agents(&self, cx: &App) -> Vec<LiveAgent> {
        let mut agents: Vec<_> = self
            .order
            .iter()
            .filter_map(|id| {
                let view = self.sessions.get(id)?.read(cx);
                let done = !view.busy && self.unseen_finished.contains(id);
                if view.summary.archived || !(view.responding() || done) {
                    return None;
                }
                let (tool, started_at, duration_ms) =
                    view.transcript.read(cx).live_turn(&view.summary.cwd);
                let needs_approval = view.needs_approval();
                Some(LiveAgent {
                    id: *id,
                    cwd: view.summary.cwd.clone(),
                    provider: view.summary.provider.clone(),
                    title: view.display_title(),
                    activity: if needs_approval {
                        "Need approval".into()
                    } else if done {
                        "Done".into()
                    } else {
                        tool.unwrap_or_else(|| "Working".into())
                    },
                    started_at,
                    duration_ms: duration_ms.filter(|_| done),
                    needs_approval,
                    done,
                })
            })
            .collect();
        // compareLiveAgents: approvals first, finished last, then oldest first.
        agents.sort_by_key(|a| (!a.needs_approval, a.done, a.started_at.unwrap_or(u64::MAX)));
        agents
    }

    pub(super) fn live_agents_preview(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let agents = if self.workspace.preferences.working_agents
            && self.workspace.sidebar_activity_visible
        {
            self.live_agents(cx)
        } else {
            Vec::new()
        };
        let shown = agents.len() >= LIVE_AGENT_MIN;
        self.live_marks
            .retain(|id, _| shown && agents.iter().any(|a| a.id == *id && !a.done));
        let ticking = shown && agents.iter().any(|a| !a.done && a.started_at.is_some());
        if !ticking {
            self.live_tick = None;
        } else if self.live_tick.is_none() {
            self.live_tick = Some(cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor()
                        .timer(std::time::Duration::from_secs(1))
                        .await;
                    if this.update(cx, |this, cx| this.notify_sidebar(cx)).is_err() {
                        break;
                    }
                }
            }));
        }
        if !shown {
            self.live_agents_expanded = false;
            return None;
        }
        let count = agents.len();
        let extra = count.saturating_sub(LIVE_AGENT_CAP);
        let expanded = self.live_agents_expanded;
        let now = monocode_core::time::now_ms();
        let cards: Vec<_> = agents
            .into_iter()
            .take(if expanded { count } else { LIVE_AGENT_CAP })
            .map(|agent| self.live_agent_card(agent, now, cx))
            .collect();
        Some(
            div()
                .id("live-agents")
                .debug_selector(|| "sidebar-activity-card".into())
                .role(Role::Group)
                .aria_label(format!("{count} working agents"))
                .flex_shrink_0()
                .px_2()
                .when(self.workspace.boxy_layout(), |d| d.pb_2())
                .child(
                    div()
                        .overflow_hidden()
                        .rounded_lg()
                        .bg(content(0.05))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .px(px(14.))
                                .py_1p5()
                                .child(
                                    div()
                                        .size(px(6.))
                                        .flex_shrink_0()
                                        .rounded_full()
                                        .bg(accent_color())
                                        .shadow(vec![
                                            BoxShadow::new(px(0.), px(0.), accent_color())
                                                .blur_radius(px(8.)),
                                        ]),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(px(12.))
                                        .text_color(content(0.5))
                                        .child("Working"),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(content(0.4))
                                        .child(count.to_string()),
                                ),
                        )
                        .child(
                            div()
                                .id("live-agents-list")
                                .flex()
                                .flex_col()
                                .gap_px()
                                .px_1()
                                .when(extra == 0, |d| d.pb_1())
                                .when(expanded, |d| d.max_h(px(360.)).overflow_y_scroll())
                                .children(cards),
                        )
                        .when(extra > 0, |d| {
                            d.child(
                                div()
                                    .id("live-agents-more")
                                    .group("live-agents-more")
                                    .role(Role::Button)
                                    .aria_expanded(expanded)
                                    .cursor_pointer()
                                    .w_full()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .gap_1()
                                    .px_2()
                                    .py_1p5()
                                    .text_size(px(11.))
                                    .text_color(content(0.5))
                                    .hover(|s| s.bg(content(0.08)).text_color(content(1.)))
                                    .child(
                                        icon(
                                            if expanded {
                                                "chevron-up"
                                            } else {
                                                "chevron-down"
                                            },
                                            12.,
                                        )
                                        .text_color(content(0.5))
                                        .group_hover("live-agents-more", |s| {
                                            s.text_color(content(1.))
                                        }),
                                    )
                                    .child(if expanded {
                                        "Show less".to_owned()
                                    } else {
                                        format!("{extra} more")
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.live_agents_expanded = !this.live_agents_expanded;
                                        this.notify_sidebar(cx);
                                    })),
                            )
                        }),
                )
                .into_any(),
        )
    }

    fn live_agent_card(
        &mut self,
        agent: LiveAgent,
        now: u64,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = agent.id;
        let project = self.project_label(&agent.cwd);
        let elapsed = if agent.done {
            agent.duration_ms.map(live_elapsed)
        } else {
            agent
                .started_at
                .map(|at| live_elapsed(now.saturating_sub(at)))
        };
        let selected = self.selected_sessions.contains(&id)
            || (self.workspace.active == Some(id) && !self.mono_active(cx));
        let status: AnyElement = if agent.needs_approval {
            icon("alert-circle", 12.)
                .text_color(rgb(AMBER))
                .into_any_element()
        } else if agent.done {
            icon("check", 12.)
                .text_color(rgb(EMERALD))
                .into_any_element()
        } else {
            let provider = agent.provider.clone();
            let mark = self
                .live_marks
                .entry(id)
                .or_insert_with(|| {
                    cx.new(|cx| {
                        let mut mark = SessionMark::tab(provider.clone());
                        mark.set(&provider, MarkState::Working, cx);
                        mark
                    })
                })
                .clone();
            div()
                .size(px(12.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .child(mark)
                .into_any_element()
        };
        let label = [
            Some(agent.title.clone()),
            Some(project.clone()),
            Some(agent.activity.clone()),
            elapsed.clone(),
        ]
        .into_iter()
        .flatten()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
        div()
            .id(("live-agent", id as usize))
            .debug_selector(move || format!("live-agent-{id}"))
            .role(Role::Button)
            .aria_label(label)
            .cursor_pointer()
            .w_full()
            .flex()
            .flex_col()
            .rounded_md()
            .px_2()
            .py_1p5()
            .when(selected, |d| d.bg(content(0.1)))
            .when(!selected, |d| d.hover(|s| s.bg(content(0.08))))
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(self.project_mark(&agent.cwd, 8.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(13.))
                            .line_height(px(18.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(content(1.))
                            .child(agent.title),
                    ),
            )
            .child(
                div()
                    .mt_1()
                    .pl_4()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .text_size(px(11.))
                    .line_height(px(14.))
                    .text_color(if agent.needs_approval {
                        rgb(AMBER).into()
                    } else if agent.done {
                        rgb(EMERALD).into()
                    } else {
                        content(0.5)
                    })
                    .child(status)
                    .child(div().min_w_0().truncate().child(agent.activity)),
            )
            .child(
                div()
                    .mt_1()
                    .pl_4()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .text_size(px(11.))
                    .line_height(px(14.))
                    .text_color(content(0.45))
                    .child(provider_icon(&agent.provider, 12.))
                    .child(div().flex_1().min_w_0().truncate().child(project))
                    .children(elapsed.map(|elapsed| div().flex_shrink_0().child(elapsed))),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.selected_sessions.clear();
                this.selection_anchor = Some(id);
                this.open(id, window, cx);
                this.invalidate_chrome(cx);
                cx.notify();
            }))
            .into_any()
    }
}
