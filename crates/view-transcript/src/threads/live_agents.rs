//! Port of src/features/sessions/ui/LiveAgentsPreview.tsx: the sidebar's
//! "Working" panel, one card per agent that is working or finished unseen.

use std::time::Duration;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Animation, AnimationExt as _, BoxShadow, Context, ElementId, EventEmitter,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled as _, Task, WeakEntity, Window, div, px,
};
use monocode_core::HarnessId;
use monocode_layout::paths::{project_key, project_name};
use monocode_layout::tab_groups::{
    JsRecord, resolve_tab_group_color, resolve_tab_group_label, resolve_tab_group_mascot,
};
use monocode_ui::widgets::spinner;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use super::parts::{css_color, eid, harness_icon, now_ms, project_mascot};

/// Fewer working agents than this hide the panel.
pub const LIVE_AGENT_MIN: usize = 2;
/// Cards shown before "N more".
pub const LIVE_AGENT_CAP: usize = 4;

/// `LiveAgent`: one row of the engine's `live_agents`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveAgent {
    pub id: String,
    pub cwd: String,
    pub title: String,
    pub harness: HarnessId,
    pub activity: String,
    pub started_at: Option<i64>,
    pub duration_ms: Option<i64>,
    pub needs_approval: bool,
    pub done: bool,
}

/// The saved project appearance (`loadTabGroupLabels` and friends), keyed
/// by project key.
#[derive(Clone, Debug, Default)]
pub struct ProjectAppearance {
    pub labels: JsRecord<String>,
    pub colors: JsRecord<usize>,
    pub custom_colors: JsRecord<String>,
    pub mascots: JsRecord<String>,
}

/// `formatLiveElapsed`.
// TODO(port): a copy of monocode_engine::attention::live_agents's version,
// because view crates may not depend on the engine. Delete it when they can.
pub fn format_live_elapsed(started_at: i64, now: i64) -> String {
    let seconds = (monocode_core::js::round((now - started_at) as f64 / 1000.0) as i64).max(1);
    if seconds < 60 {
        return format!("{seconds}s");
    }
    let minutes = seconds / 60;
    let rest = seconds % 60;
    if minutes < 60 {
        return if rest > 0 {
            format!("{minutes}m {rest}s")
        } else {
            format!("{minutes}m")
        };
    }
    let hours = minutes / 60;
    let min_rest = minutes % 60;
    if min_rest > 0 {
        format!("{hours}h {min_rest}m")
    } else {
        format!("{hours}h")
    }
}

/// What one card shows.
#[derive(Clone, Debug, PartialEq)]
pub struct LiveAgentCard {
    pub id: String,
    pub title: String,
    /// `projectName(cwd)`: the mascot's seed.
    pub seed: String,
    pub project: String,
    pub color: String,
    pub mascot: Option<String>,
    pub activity: String,
    pub elapsed: String,
    pub live: bool,
    pub needs_approval: bool,
    pub done: bool,
    pub harness: HarnessId,
}

impl LiveAgentCard {
    /// The card's accessible name: title, project, activity, elapsed.
    pub fn label(&self) -> String {
        [
            self.title.as_str(),
            self.project.as_str(),
            self.activity.as_str(),
            self.elapsed.as_str(),
        ]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
    }
}

/// `LiveAgentCard`'s derived values.
pub fn live_agent_card(
    agent: &LiveAgent,
    now: i64,
    appearance: &ProjectAppearance,
) -> LiveAgentCard {
    let seed = project_name(&agent.cwd);
    let key = project_key(&agent.cwd);
    let project = resolve_tab_group_label(&key, Some(&appearance.labels), &seed);
    let color = resolve_tab_group_color(
        &key,
        Some(&appearance.colors),
        Some(&appearance.custom_colors),
        Some(&seed),
    );
    let elapsed = if agent.done {
        agent
            .duration_ms
            .map(|duration| format_live_elapsed(0, duration))
            .unwrap_or_default()
    } else {
        agent
            .started_at
            .map(|started| format_live_elapsed(started, now))
            .unwrap_or_default()
    };
    let activity = if agent.needs_approval {
        "Need approval".to_string()
    } else if agent.done {
        "Done".to_string()
    } else {
        agent.activity.clone()
    };
    LiveAgentCard {
        id: agent.id.clone(),
        title: agent.title.clone(),
        mascot: resolve_tab_group_mascot(&key, Some(&appearance.mascots)),
        seed,
        project,
        color,
        activity,
        elapsed,
        live: !agent.needs_approval && !agent.done,
        needs_approval: agent.needs_approval,
        done: agent.done,
        harness: agent.harness,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LiveAgentsPreviewEvent {
    /// `onSelect`.
    Select(String),
}

/// The panel.
pub struct LiveAgentsPreview {
    agents: Vec<LiveAgent>,
    active_session_id: Option<String>,
    bottom_spacing: bool,
    appearance: ProjectAppearance,
    expanded: bool,
    now: i64,
    /// Freezes the clock, for screenshots and tests.
    fixed_now: Option<i64>,
    scroll: ScrollHandle,
    ticker: Option<Task<()>>,
}

impl EventEmitter<LiveAgentsPreviewEvent> for LiveAgentsPreview {}

impl LiveAgentsPreview {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let _ = cx;
        Self {
            agents: Vec::new(),
            active_session_id: None,
            bottom_spacing: false,
            appearance: ProjectAppearance::default(),
            expanded: false,
            now: now_ms(),
            fixed_now: None,
            scroll: ScrollHandle::new(),
            ticker: None,
        }
    }

    pub fn set_agents(&mut self, agents: Vec<LiveAgent>, cx: &mut Context<Self>) {
        self.agents = agents;
        self.sync_ticker(cx);
        cx.notify();
    }

    pub fn set_active_session_id(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        self.active_session_id = id;
        cx.notify();
    }

    pub fn set_bottom_spacing(&mut self, spacing: bool, cx: &mut Context<Self>) {
        self.bottom_spacing = spacing;
        cx.notify();
    }

    pub fn set_appearance(&mut self, appearance: ProjectAppearance, cx: &mut Context<Self>) {
        self.appearance = appearance;
        cx.notify();
    }

    /// Pins the clock, for screenshots and tests.
    pub fn set_now(&mut self, now: Option<i64>, cx: &mut Context<Self>) {
        self.fixed_now = now;
        if let Some(now) = now {
            self.now = now;
        }
        self.sync_ticker(cx);
        cx.notify();
    }

    pub fn toggle_expanded(&mut self, cx: &mut Context<Self>) {
        self.expanded = !self.expanded;
        cx.notify();
    }

    pub fn select(&mut self, id: &str, cx: &mut Context<Self>) {
        cx.emit(LiveAgentsPreviewEvent::Select(id.to_string()));
    }

    /// Whether the panel shows at all.
    pub fn is_shown(&self) -> bool {
        self.agents.len() >= LIVE_AGENT_MIN
    }

    /// `agents.length - LIVE_AGENT_CAP`.
    pub fn extra(&self) -> isize {
        self.agents.len() as isize - LIVE_AGENT_CAP as isize
    }

    /// The cards on screen, in the incoming order.
    pub fn visible_cards(&self) -> Vec<LiveAgentCard> {
        if !self.is_shown() {
            return Vec::new();
        }
        let visible = if self.expanded || self.extra() <= 0 {
            &self.agents[..]
        } else {
            &self.agents[..LIVE_AGENT_CAP]
        };
        visible
            .iter()
            .map(|agent| live_agent_card(agent, self.now, &self.appearance))
            .collect()
    }

    /// The live region: the count only, so ticking timers stay out of it.
    pub fn live_region(&self) -> String {
        format!("{} working agents", self.agents.len())
    }

    /// The "N more" or "Show less" button, when the list is capped.
    pub fn more_label(&self) -> Option<String> {
        let extra = self.extra();
        (extra > 0).then(|| {
            if self.expanded {
                "Show less".to_string()
            } else {
                format!("{extra} more")
            }
        })
    }

    fn ticking(&self) -> bool {
        self.fixed_now.is_none()
            && self.agents.len() >= LIVE_AGENT_MIN
            && self
                .agents
                .iter()
                .any(|agent| !agent.done && agent.started_at.is_some())
    }

    fn sync_ticker(&mut self, cx: &mut Context<Self>) {
        if !self.ticking() {
            self.ticker = None;
            return;
        }
        if self.ticker.is_some() {
            return;
        }
        self.now = now_ms();
        self.ticker = Some(cx.spawn(async move |this: WeakEntity<Self>, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let alive = this.update(cx, |this, cx| {
                    this.now = now_ms();
                    cx.notify();
                });
                if alive.is_err() {
                    break;
                }
            }
        }));
    }

    fn render_card(
        &self,
        card: LiveAgentCard,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let selected = self.active_session_id.as_deref() == Some(card.id.as_str());
        let color = css_color(&card.color).unwrap_or(theme.colors.content);
        let status_color = if card.needs_approval {
            theme.colors.warning
        } else if card.done {
            theme.colors.success
        } else {
            theme.content(0.50)
        };
        let status_icon = if card.needs_approval {
            icon(IconName::CircleAlert)
                .size(u(12.))
                .text_color(status_color)
                .into_any_element()
        } else if card.done {
            icon(IconName::Check)
                .size(u(12.))
                .text_color(status_color)
                .into_any_element()
        } else {
            spinner(eid(&card.id, "spinner"))
                .color(status_color)
                .into_any_element()
        };
        let hover = theme.content(0.08);
        let id = card.id.clone();
        div()
            .id(ElementId::Name(SharedString::from(format!(
                "live-agent:{}",
                card.id
            ))))
            .relative()
            .flex()
            .w_full()
            .flex_col()
            .rounded(u(theme.radius.md))
            .px(u(8.))
            .py(u(6.))
            .when(selected, |el| el.bg(theme.colors.selection))
            .when(!selected, |el| el.hover(move |style| style.bg(hover)))
            .on_click(cx.listener(move |this, _, _, cx| this.select(&id, cx)))
            .child(
                div()
                    .flex()
                    .min_w_0()
                    .items_center()
                    .gap(u(8.))
                    .child(div().flex_none().size(u(8.)).child(project_mascot(
                        eid(&card.id, "mascot"),
                        &card.seed,
                        card.mascot.as_deref(),
                        color,
                        card.live,
                    )))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_px(13.)
                            .semibold()
                            .leading(theme.leading.snug)
                            .text_color(theme.colors.content)
                            .child(card.title.clone()),
                    ),
            )
            .child(
                div()
                    .mt(u(4.))
                    .flex()
                    .min_w_0()
                    .items_center()
                    .gap(u(6.))
                    .pl(u(16.))
                    .text_px(11.)
                    .leading(theme.leading.tight)
                    .text_color(status_color)
                    .child(status_icon)
                    .child(div().min_w_0().truncate().child(card.activity.clone())),
            )
            .child(
                div()
                    .mt(u(4.))
                    .flex()
                    .min_w_0()
                    .items_center()
                    .gap(u(6.))
                    .pl(u(16.))
                    .text_px(11.)
                    .leading(theme.leading.tight)
                    .text_color(theme.content(0.45))
                    .child(harness_icon(card.harness, 12.))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .child(card.project.clone()),
                    )
                    .when(!card.elapsed.is_empty(), |el| {
                        el.child(div().flex_none().tabular().child(card.elapsed.clone()))
                    }),
            )
    }
}

/// The header dot: `bg-accent` with an 8px glow, pulsing.
fn working_dot(theme: &Theme) -> impl IntoElement + use<> {
    let accent = theme.colors.accent;
    div()
        .flex_none()
        .size(u(6.))
        .rounded_full()
        .bg(accent)
        .shadow(vec![
            BoxShadow::new(px(0.), px(0.), accent).blur_radius(px(8.)),
        ])
        .with_animation(
            "live-agents-pulse",
            Animation::new(Duration::from_secs(2)).repeat(),
            // Tailwind `animate-pulse`: 1, 0.5 at the half, 1.
            |el, t| {
                let dip = if t < 0.5 { t * 2. } else { (1. - t) * 2. };
                el.opacity(1. - 0.5 * dip)
            },
        )
}

impl Render for LiveAgentsPreview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.is_shown() {
            return div().into_any_element();
        }
        let theme = Theme::of(cx).clone();
        let extra = self.extra();
        let cards = self.visible_cards();
        let mut list = div()
            .id("live-agents-list")
            .flex()
            .flex_col()
            .gap(px(1.))
            .px(u(4.))
            .when(extra <= 0, |el| el.pb(u(4.)));
        if self.expanded {
            list = list
                .max_h(window.viewport_size().height * 0.45)
                .overflow_y_scroll()
                .track_scroll(&self.scroll);
        }
        for card in cards {
            list = list.child(self.render_card(card, &theme, cx));
        }
        let hover = theme.content(0.08);
        let ink = theme.colors.content;
        let more = self.more_label().map(|label| {
            div()
                .id("live-agents-more")
                .flex()
                .w_full()
                .items_center()
                .justify_center()
                .gap(u(4.))
                .px(u(8.))
                .py(u(6.))
                .text_px(11.)
                .text_color(theme.content(0.50))
                .hover(move |style| style.bg(hover).text_color(ink))
                .on_click(cx.listener(|this, _, _, cx| this.toggle_expanded(cx)))
                .child(
                    icon(if self.expanded {
                        IconName::ChevronUp
                    } else {
                        IconName::ChevronDown
                    })
                    .size(u(12.))
                    .text_color(theme.content(0.50)),
                )
                .child(label)
        });
        div()
            .flex_none()
            .px(u(8.))
            .when(self.bottom_spacing, |el| el.pb(u(8.)))
            .child(
                div()
                    .overflow_hidden()
                    .rounded(u(theme.radius.lg))
                    .bg(theme.content(0.05))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(u(8.))
                            .px(u(14.))
                            .py(u(6.))
                            .child(working_dot(&theme))
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .truncate()
                                    .text_px(12.)
                                    .text_color(theme.content(0.50))
                                    .child("Working"),
                            )
                            .child(
                                div()
                                    .text_px(11.)
                                    .tabular()
                                    .text_color(theme.content(0.40))
                                    .child(self.agents.len().to_string()),
                            ),
                    )
                    .child(list)
                    .children(more),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{AppContext as _, TestAppContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    fn agent(id: &str, cwd: &str) -> LiveAgent {
        LiveAgent {
            id: id.into(),
            cwd: cwd.into(),
            title: format!("Agent {id}"),
            harness: HarnessId::Codex,
            activity: "Working".into(),
            started_at: Some(1_000),
            duration_ms: None,
            needs_approval: false,
            done: false,
        }
    }

    fn init(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            monocode_ui::init(monocode_ui::AppearanceSettings::default(), cx);
        });
    }

    #[gpui::test]
    fn keeps_the_incoming_activity_order_and_the_global_four_agent_cap(cx: &mut TestAppContext) {
        init(cx);
        let mut labels = JsRecord::new();
        labels.insert("/repo/a", "Alpha".to_string());
        labels.insert("/repo/b", "Beta".to_string());
        let preview = cx.new(|cx| {
            let mut preview = LiveAgentsPreview::new(cx);
            preview.set_now(Some(5_000), cx);
            preview.set_appearance(
                ProjectAppearance {
                    labels,
                    ..Default::default()
                },
                cx,
            );
            let mut waiting = agent("b-1", "/repo/b");
            waiting.needs_approval = true;
            preview.set_agents(
                vec![
                    agent("a-1", "/repo/a"),
                    waiting,
                    agent("a-2", "/repo/a"),
                    agent("b-2", "/repo/b"),
                    agent("c-1", "/repo/c"),
                ],
                cx,
            );
            preview
        });
        preview.read_with(cx, |preview, _| {
            let cards = preview.visible_cards();
            let ids: Vec<&str> = cards.iter().map(|card| card.id.as_str()).collect();
            assert_eq!(ids, ["a-1", "b-1", "a-2", "b-2"]);
            assert!(cards[0].label().contains("Alpha"));
            assert!(cards[1].label().contains("Beta"));
            // The working agent's mascot hops; the one waiting does not.
            assert!(cards[0].live);
            assert!(!cards[1].live);
            assert_eq!(cards[1].activity, "Need approval");
            assert_eq!(preview.more_label().as_deref(), Some("1 more"));
        });
        preview.update(cx, |preview, cx| preview.toggle_expanded(cx));
        preview.read_with(cx, |preview, _| {
            assert_eq!(preview.visible_cards().len(), 5);
            assert_eq!(preview.more_label().as_deref(), Some("Show less"));
        });
    }

    #[gpui::test]
    fn selects_an_agent_from_the_activity_list(cx: &mut TestAppContext) {
        init(cx);
        let selected = Rc::new(RefCell::new(Vec::new()));
        let sink = selected.clone();
        let preview = cx.new(|cx| {
            let mut preview = LiveAgentsPreview::new(cx);
            preview.set_agents(vec![agent("a", "/repo/a"), agent("b", "/repo/b")], cx);
            preview
        });
        cx.update(|cx| {
            cx.subscribe(&preview, move |_, event: &LiveAgentsPreviewEvent, _| {
                sink.borrow_mut().push(event.clone());
            })
            .detach();
        });
        preview.update(cx, |preview, cx| preview.select("b", cx));
        cx.run_until_parked();
        assert_eq!(
            *selected.borrow(),
            [LiveAgentsPreviewEvent::Select("b".into())]
        );
    }

    #[gpui::test]
    fn keeps_elapsed_timers_out_of_its_live_region(cx: &mut TestAppContext) {
        init(cx);
        let preview = cx.new(|cx| {
            let mut preview = LiveAgentsPreview::new(cx);
            preview.set_now(Some(61_000), cx);
            preview.set_agents(vec![agent("a", "/repo/a"), agent("b", "/repo/b")], cx);
            preview
        });
        preview.read_with(cx, |preview, _| {
            assert_eq!(preview.live_region(), "2 working agents");
            assert_eq!(preview.visible_cards()[0].elapsed, "1m");
        });
    }

    #[test]
    fn labels_a_finished_agent_by_its_folder() {
        let appearance = ProjectAppearance::default();
        let card = live_agent_card(
            &LiveAgent {
                done: true,
                duration_ms: Some(95_000),
                ..agent("a", "/Users/me/arcade")
            },
            0,
            &appearance,
        );
        assert_eq!(card.project, "arcade");
        assert_eq!(card.activity, "Done");
        assert_eq!(card.elapsed, "1m 35s");
    }
}
