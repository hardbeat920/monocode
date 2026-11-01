//! Session cards in the sidebar, and the summary a lead's row gives of its
//! subagents. Subagents (threads a lead drives, and an Orchestrator's agents)
//! are never rows of their own: while their lead is the focused chat they
//! branch off its card on a curved tree, and open beside it. Threads another
//! thread only opened are ordinary rows.
use super::*;
use crate::reveal::bezier;
use std::time::{Duration, Instant};

/// RevealHeight's `reveal-height-in` and `reveal-height-out`.
pub(super) const OPEN: Duration = Duration::from_millis(240);
pub(super) const CLOSE: Duration = Duration::from_millis(180);
/// More agents than this show only the count on their lead.
const PROGRESS_PILLS: usize = 8;
/// A sidebar narrower than this shows only the count.
const PROGRESS_PILLS_WIDTH: f32 = 300.;
/// One subagent on the lead's tree.
const TREE_ROW: f32 = 24.;
/// Space between the lead's last line and its tree.
const TREE_TOP: f32 = 4.;
/// From the trunk to where a subagent's mark starts.
const TREE_INDENT: f32 = 16.;
/// The elbow's corner, as the card's own corners.
const TREE_CURVE: f32 = 8.;

/// Each lead's subagent tree, growing open while it is the focused chat.
#[derive(Default)]
pub(crate) struct Rails {
    motions: HashMap<i64, Motion>,
    frames: Option<gpui::Task<()>>,
}

#[derive(Clone, Copy)]
pub(super) struct Motion {
    pub(super) open: bool,
    pub(super) from: f32,
    pub(super) start: Instant,
}
impl Motion {
    pub(super) fn value(&self, now: Instant) -> f32 {
        let (duration, curve) = if self.open {
            (OPEN, (0.2, 0., 0., 1.))
        } else {
            (CLOSE, (0.4, 0., 1., 1.))
        };
        let t = (now.saturating_duration_since(self.start).as_secs_f32() / duration.as_secs_f32())
            .clamp(0., 1.);
        let eased = bezier(t, curve.0, curve.1, curve.2, curve.3);
        let target = if self.open { 1. } else { 0. };
        self.from + (target - self.from) * eased
    }
    pub(super) fn settled(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.start) >= if self.open { OPEN } else { CLOSE }
    }
}

/// Where a subagent stands, as the lead's progress pills show it.
struct Standing {
    tone: gpui::Hsla,
    alert: bool,
    done: bool,
}

/// What a lead's sidebar row summarizes.
pub(crate) enum Rail {
    /// Its subagents: threads it drives, or its Orchestrator agents.
    Agents(monocode_core::orchestration::Summary),
}

/// `.zen-phase-step.orchestration-tree-step`: a step branching off the
/// transcript's tool rail, its row 2px lower inside the step's padding.
pub(super) fn step(last: bool, child: impl IntoElement) -> gpui::Div {
    div()
        .relative()
        .pl(px(20.))
        .py(px(2.))
        .child(crate::activity::rail_branch_at(last, true, 2.))
        .child(child)
}

impl Shell {
    /// Orchestrated workers live on their lead's rail, never in the list.
    pub(super) fn orchestration_worker(&self, id: i64) -> bool {
        self.orchestration_lead(id).is_some()
    }
    /// The lead whose run a worker session belongs to. A thread another
    /// started is a session of its own, even though its lead tracks it.
    pub(super) fn orchestration_lead(&self, id: i64) -> Option<i64> {
        if self.thread_parents.contains_key(&id) {
            return None;
        }
        self.orchestration_summaries
            .iter()
            .find(|(_, s)| s.tasks.iter().any(|t| t.session == Some(id)))
            .map(|(lead, _)| *lead)
    }
    /// The lead's orchestrated agents, without the threads it started.
    /// What the lead's row summarizes: all of its subagents.
    fn rail(&self, id: i64) -> Option<Rail> {
        let summary = self.orchestration_summaries.get(&id)?;
        (!summary.tasks.is_empty()).then(|| Rail::Agents(summary.clone()))
    }
    /// A lead shows its subagents while it, or one of them, is the chat in
    /// front.
    fn rail_open(&self, lead: i64, cx: &App) -> bool {
        if self.mono_active(cx) || self.app_view_open() {
            return false;
        }
        self.workspace
            .active
            .is_some_and(|active| active == lead || self.subagent_lead(active) == Some(lead))
    }
    /// Moves each lead's tree toward its open state. A tree the sidebar first
    /// shows is simply there; later changes grow and fold.
    pub(super) fn sync_rails(&mut self, ids: &[i64], cx: &mut Context<Self>) {
        let now = Instant::now();
        let reduced = cx.reduce_motion();
        let mut animating = false;
        let mut seen = HashSet::new();
        for &id in ids {
            if self.rail(id).is_none() {
                continue;
            }
            seen.insert(id);
            let open = self.rail_open(id, cx);
            match self.rails.motions.get(&id).copied() {
                None => {
                    self.rails.motions.insert(
                        id,
                        Motion {
                            open,
                            from: f32::from(open),
                            start: now - OPEN,
                        },
                    );
                }
                Some(motion) if motion.open != open => {
                    self.rails.motions.insert(
                        id,
                        Motion {
                            open,
                            from: if reduced {
                                f32::from(open)
                            } else {
                                motion.value(now)
                            },
                            start: if reduced { now - OPEN } else { now },
                        },
                    );
                }
                _ => {}
            }
            animating |= !self.rails.motions[&id].settled(now);
        }
        self.rails.motions.retain(|id, _| seen.contains(id));
        if animating && self.rails.frames.is_none() {
            self.rails.frames = Some(cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.rails.frames = None;
                    this.notify_sidebar(cx);
                });
            }));
        }
    }
    /// How far the lead's tree is open, 0 to 1.
    fn rail_progress(&self, lead: i64, cx: &App) -> f32 {
        match self.rails.motions.get(&lead) {
            Some(motion) => motion.value(Instant::now()),
            None => f32::from(self.rail_open(lead, cx)),
        }
    }
    /// The lead's subagents on a trunk under `trunk_x`, each branching off
    /// on a curved elbow; the open share of its height, or nothing when shut.
    pub(super) fn rail_tree(
        &self,
        lead: i64,
        rail: Option<&Rail>,
        trunk_x: f32,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let Rail::Agents(summary) = rail?;
        let progress = self.rail_progress(lead, cx);
        if progress <= 0. {
            return None;
        }
        use crate::conversation::subagents::{SubagentState, mark};
        let line = content(0.16);
        let count = summary.tasks.len();
        let height = TREE_TOP + count as f32 * TREE_ROW;
        let rows = summary.tasks.iter().enumerate().map(|(ix, task)| {
            let needs_input = task.session.is_some_and(|s| {
                self.sessions
                    .get(&s)
                    .is_some_and(|v| v.read(cx).needs_approval())
            });
            let state = SubagentState::of(&task.status, summary.live, needs_input);
            let tone: gpui::Hsla = match state {
                SubagentState::Working => accent_color(),
                SubagentState::NeedsYou => rgb(0xfbbf24).into(),
                SubagentState::Failed => rgb(0xf87171).into(),
                SubagentState::Done => rgb(0x34d399).into(),
                SubagentState::Waiting => content(0.45),
            };
            let first = ix == 0;
            let last = ix + 1 == count;
            // The first elbow reaches up to the lead's line through the gap.
            let reach = if first { TREE_TOP } else { 0. };
            let session = task.session;
            div()
                .id(("rail-subagent", ix))
                .debug_selector(move || format!("rail-subagent-{lead}-{ix}"))
                .relative()
                .w_full()
                .h(px(TREE_ROW))
                .pl(px(trunk_x + TREE_INDENT))
                .flex()
                .items_center()
                // The trunk carries on past every subagent but the last.
                .when(!last, |d| {
                    d.child(
                        div()
                            .absolute()
                            .left(px(trunk_x))
                            .top(px(-reach))
                            .bottom_0()
                            .w(px(1.))
                            .bg(line),
                    )
                })
                .child(
                    div()
                        .absolute()
                        .left(px(trunk_x))
                        .top(px(-reach))
                        .w(px(TREE_INDENT - 4.))
                        .h(px(reach + TREE_ROW / 2.))
                        .border_l_1()
                        .border_b_1()
                        .border_color(line)
                        .rounded_bl(px(TREE_CURVE)),
                )
                .child(
                    div()
                        .id(("rail-subagent-open", ix))
                        .role(Role::Button)
                        .aria_label(format!("Open subagent {}, {}", task.title, state.label()))
                        .flex_1()
                        .min_w_0()
                        .h(px(TREE_ROW))
                        .px_1()
                        .rounded(px(6.))
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_size(px(12.))
                        .text_color(content(0.75))
                        .when(session.is_some(), |d| {
                            d.cursor_pointer()
                                .hover(|s| s.bg(content(0.07)).text_color(content(1.)))
                        })
                        .child(mark(lead as usize * 64 + ix, state))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .child(task.title.clone()),
                        )
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_size(px(11.))
                                .text_color(tone)
                                .child(state.label()),
                        )
                        // The lead's card must not take the press as its own.
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .when_some(session, |d, session| {
                            d.on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.open(session, window, cx);
                            }))
                        }),
                )
        });
        Some(
            div()
                .id(("rail-tree", lead as usize))
                .debug_selector(move || format!("rail-tree-{lead}"))
                .role(Role::Group)
                .aria_label("Subagents")
                .w_full()
                .h(px(height * progress))
                .flex_shrink_0()
                .overflow_hidden()
                .opacity(progress)
                .child(div().pt(px(TREE_TOP)).flex().flex_col().children(rows))
                .into_any_element(),
        )
    }
    /// The height the lead's tree takes now, for rows of a fixed height.
    pub(super) fn rail_tree_height(&self, lead: i64, rail: Option<&Rail>, cx: &App) -> f32 {
        match rail {
            Some(Rail::Agents(summary)) => {
                (TREE_TOP + summary.tasks.len() as f32 * TREE_ROW) * self.rail_progress(lead, cx)
            }
            None => 0.,
        }
    }
    fn standing(
        &self,
        summary: &monocode_core::orchestration::Summary,
        task: &monocode_core::orchestration::SummaryTask,
        cx: &App,
    ) -> Standing {
        let needs_input = task.session.is_some_and(|s| {
            self.sessions
                .get(&s)
                .is_some_and(|v| v.read(cx).needs_approval())
        });
        let working = summary.live && task.status == "running" && !needs_input;
        let alert =
            needs_input || matches!(task.status.as_str(), "failed" | "blocked" | "interrupted");
        let done = task.status == "completed";
        let tone = if alert {
            rgb(0xfbbf24).into()
        } else if working {
            accent_color()
        } else if done {
            rgb(0x34d399).into()
        } else {
            content(0.45)
        };
        Standing { tone, alert, done }
    }
    /// The lead's share of its agents that are done: a pill each, then 3/4.
    /// A compact row, or a narrow sidebar, shows only the count so the title
    /// and branch keep their room.
    pub(super) fn agent_progress(
        &self,
        lead: i64,
        summary: &monocode_core::orchestration::Summary,
        compact: bool,
        cx: &App,
    ) -> AnyElement {
        let standings: Vec<Standing> = summary
            .tasks
            .iter()
            .map(|task| self.standing(summary, task, cx))
            .collect();
        let done = standings.iter().filter(|s| s.done && !s.alert).count();
        div()
            .id(("rail-progress", lead as usize))
            .debug_selector(move || format!("rail-progress-{lead}"))
            .role(Role::Image)
            .aria_label(format!("{done} of {} agents done", standings.len()))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(6.))
            .when(
                !compact
                    && self.workspace.sidebar_width >= PROGRESS_PILLS_WIDTH
                    && standings.len() <= PROGRESS_PILLS,
                |d| {
                    d.child(div().flex().gap(px(2.)).children(standings.iter().map(|s| {
                        div()
                            .w(px(8.))
                            .h(px(4.))
                            .rounded(px(2.))
                            .bg(if s.done || s.alert {
                                s.tone
                            } else {
                                content(0.18)
                            })
                    })))
                },
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(content(0.45))
                    .child(format!("{done}/{}", standings.len())),
            )
            .into_any_element()
    }
    /// A session row and, under it, whatever rail it has.
    pub(super) fn session_card(&self, id: i64, cx: &mut Context<Self>) -> AnyElement {
        self.session_card_at_depth(id, false, cx)
    }

    /// Folder members use the same compact row as an Operator's children,
    /// while keeping any Operator or Orchestrator rail of their own.
    pub(super) fn nested_session_card(&self, id: i64, cx: &mut Context<Self>) -> AnyElement {
        self.session_card_at_depth(id, true, cx)
    }

    fn session_card_at_depth(&self, id: i64, nested: bool, cx: &mut Context<Self>) -> AnyElement {
        if self.workspace.classic_layout {
            return self.classic_session_card_at_depth(id, nested, false, cx);
        }
        let rail = self.rail(id);
        if rail.is_none() && !nested {
            return self.session_row(id, cx);
        }
        self.session_row_with(id, nested, rail.as_ref(), cx)
    }

    /// The original sidebar uses the full, three-line SessionCard. Folder
    /// members omit only the model header. Orchestrated agents are contained
    /// within their lead's card, between its title and workspace footer.
    pub(super) fn classic_session_card_at_depth(
        &self,
        id: i64,
        nested: bool,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let rail = self.rail(id);
        self.classic_session_row_with(id, nested, compact, rail.as_ref(), cx)
    }

    fn classic_session_row_with(
        &self,
        id: i64,
        nested: bool,
        compact: bool,
        rail: Option<&Rail>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let Some(row) = self.rename_row(&session_groups::RenameTarget::Session(id), cx) {
            return row;
        }
        let work_item = self.work_item_badge(id, false, cx);
        let session = self.session_metadata(id, cx);
        let selected = self.selected_sessions.contains(&id);
        let active =
            self.workspace.active == Some(id) && !self.mono_active(cx) && !self.app_view_open();
        let draft = session.has_saved_draft();
        let approval = session.needs_approval();
        let header = !compact;
        let saved = self.session_details.get(&id);
        let (model, branch) = self
            .sessions
            .get(&id)
            .and_then(|view| view.read(cx).sidebar_details())
            .unwrap_or_else(|| {
                (
                    crate::pickers::cached_model_name(
                        Kind::from_id(&session.summary.provider),
                        saved.and_then(|details| details.model.as_deref()),
                    ),
                    None,
                )
            });
        let cwd = self
            .session_worktrees
            .get(&id)
            .and_then(|cwd| cwd.as_deref())
            .unwrap_or(&session.summary.cwd);
        let branch = branch
            .or_else(|| saved.and_then(|details| details.branch.clone()))
            .or_else(|| {
                let git = self.git.read(cx);
                (git.root == cwd)
                    .then(|| git.index.as_ref().and_then(|index| index.branch.clone()))
                    .flatten()
            });
        let repo = std::path::Path::new(&session.summary.cwd)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&session.summary.cwd);
        let git_label = if saved.is_some_and(|details| details.worktree_removed) {
            "No branch".to_string()
        } else {
            match branch.filter(|branch| !branch.is_empty()) {
                Some(branch) if !repo.is_empty() => format!("{repo}/{branch}"),
                Some(branch) => branch,
                None => repo.to_owned(),
            }
        };
        let model = if session.summary.provider == "claude" {
            model
                .strip_prefix("Claude ")
                .map(|name| SharedString::from(name.to_owned()))
                .unwrap_or(model)
        } else {
            model
        };
        let status = self.classic_session_status(id, &session, cx);
        let title = div()
            .debug_selector(move || format!("session-title-{id}"))
            .flex_1()
            .min_w_0()
            .h(px(17.875))
            .text_size(px(13.))
            .line_height(px(17.875))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(content(1.))
            .child(crate::particle_text::particle_text(session.display_title()));
        let body = div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .when(header, |d| {
                d.child(
                    div()
                        .h(px(16.5))
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .items_center()
                                .gap_1p5()
                                .child(provider_icon(&session.summary.provider, 14.))
                                .child(
                                    div()
                                        .debug_selector(move || format!("session-model-{id}"))
                                        .min_w_0()
                                        .truncate()
                                        .text_size(px(11.))
                                        .line_height(px(16.5))
                                        .text_color(content(0.5))
                                        .child(model),
                                ),
                        )
                        .child(status),
                )
            })
            .child(
                div()
                    .when(header, |d| d.mt_1())
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .when(session.summary.pinned, |d| {
                        d.child(icon("pin", 12.).text_color(content(0.45)))
                    })
                    .child(title)
                    .when(!header, |d| {
                        d.child(
                            div()
                                .pl_2()
                                .child(self.classic_session_status(id, &session, cx)),
                        )
                    }),
            )
            .children(self.started_from_line(id, cx).map(|line| line.mt_1()))
            .child(
                div()
                    .mt_1()
                    .h(px(20.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap_1()
                            .text_size(px(11.))
                            .line_height(px(16.5))
                            .text_color(content(0.45))
                            .when(!git_label.is_empty(), |d| {
                                d.child(icon("git-branch", 12.)).child(
                                    div()
                                        .debug_selector(move || format!("session-workspace-{id}"))
                                        .min_w_0()
                                        .truncate()
                                        .child(git_label),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap_px()
                            .child(
                                div()
                                    .size(px(20.))
                                    .when(self.session_hover == Some(id), |d| {
                                        d.child(self.session_archive_button(id, &session, cx))
                                    }),
                            )
                            .children(work_item)
                            .when(
                                self.automation_state.session_automations.contains_key(&id),
                                |d| d.child(icon("zap", 12.).text_color(rgb(0xfbbf24))),
                            )
                            .when(rail.is_some(), |d| {
                                d.child(icon("share", 13.).text_color(content(0.5)))
                            }),
                    ),
            );
        let anchor = self
            .session_row_bounds
            .get(&id)
            .cloned()
            .unwrap_or_default();
        self.sidebar_list.track_anchor(anchor.clone());
        let mut selected_fill = crate::theme::accent_color();
        selected_fill.a = 0.15;
        div()
            .id(("sidebar-session", id as usize))
            .debug_selector(move || format!("sidebar-session-{id}"))
            .group("session-row")
            .role(Role::Button)
            .aria_label(session.display_title())
            .aria_selected(selected || active)
            .aria_description(format!(
                "{} conversation",
                Kind::from_id(&session.summary.provider).label()
            ))
            .cursor_pointer()
            .relative()
            .w_full()
            .border_1()
            .border_color(content(0.))
            .rounded_md()
            .px(px(if nested { 6. } else { 10. }))
            .pt(px(if compact { 6. } else { 8. }))
            .pb(px(if compact { 6. } else { 8. }))
            .flex()
            .flex_col()
            .text_color(content(0.8))
            .when(selected, |d| d.bg(selected_fill))
            .when(!selected && approval, |d| d.bg(content(0.2)))
            .when(!selected && !approval && active, |d| d.bg(content(0.1)))
            .when(draft || approval, |d| {
                d.border_dashed()
                    .border_color(content(if active || selected || approval {
                        0.3
                    } else {
                        0.25
                    }))
            })
            .when(!selected && !approval && !active, |d| {
                d.hover(|s| s.bg(content(0.05)))
            })
            .when(
                self.sidebar_drop == Some(session_groups::ListDrop::Session(id)),
                |d| {
                    let mut fill = crate::theme::accent_color();
                    fill.a = 0.2;
                    d.bg(fill)
                },
            )
            .when(self.deleting.contains(&id), |d| d.opacity(0.5))
            .child(body)
            .children(self.rail_tree(id, rail, 6., cx))
            .child(anchor.probe())
            .on_hover(cx.listener(move |this, hovered, window, cx| {
                // Original cards carry their details inline, so hovering only
                // warms the existing metadata path; it does not open a preview.
                this.hover_session(id, anchor.clone(), *hovered, false, window, cx);
            }))
            .on_click(
                cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                    if std::mem::take(&mut this.skip_session_click) {
                        return;
                    }
                    this.select_sidebar_session(id, event.modifiers(), window, cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, _| {
                    this.skip_session_click = false;
                    this.tab_drag = None;
                    this.pane_drag = Some(PaneDrag {
                        source: DragSource::Session(id),
                        start: event.position,
                        position: event.position,
                        active: false,
                        over: None,
                        insert: None,
                    });
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    this.context_selection = !this.selected_sessions.contains(&id);
                    if this.context_selection {
                        this.selected_sessions.clear();
                        this.selected_sessions.insert(id);
                    }
                    this.open_menu(
                        MenuTarget::Session(id),
                        Anchor::Point(event.position),
                        window,
                        cx,
                    );
                }),
            )
            .into_any_element()
    }

    /// The original search applies the same fuzzy query independently to the
    /// title, model, harness, and repository/branch fields.
    pub(super) fn classic_session_matches_query(&self, id: i64, cx: &App) -> bool {
        let query = self.query.trim();
        if query.is_empty() {
            return true;
        }
        let session = self.session_metadata(id, cx);
        let saved = self.session_details.get(&id);
        let live = self
            .sessions
            .get(&id)
            .and_then(|view| view.read(cx).sidebar_details());
        let model = live
            .as_ref()
            .map(|(model, _)| model.to_string())
            .unwrap_or_else(|| {
                crate::pickers::cached_model_name(
                    Kind::from_id(&session.summary.provider),
                    saved.and_then(|details| details.model.as_deref()),
                )
                .to_string()
            });
        let cwd = self
            .session_worktrees
            .get(&id)
            .and_then(|cwd| cwd.as_deref())
            .unwrap_or(&session.summary.cwd);
        let branch = live
            .and_then(|(_, branch)| branch)
            .or_else(|| saved.and_then(|details| details.branch.clone()))
            .or_else(|| {
                let git = self.git.read(cx);
                (git.root == cwd)
                    .then(|| git.index.as_ref().and_then(|index| index.branch.clone()))
                    .flatten()
            });
        let repo = std::path::Path::new(&session.summary.cwd)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        let git = match branch {
            Some(branch) if !repo.is_empty() => format!("{repo}/{branch}"),
            Some(branch) => branch,
            None => repo.to_owned(),
        };
        [
            session.display_title(),
            session.summary.title.clone(),
            saved
                .and_then(|details| details.model.clone())
                .unwrap_or_default(),
            model,
            session.summary.provider.clone(),
            git,
        ]
        .iter()
        .any(|field| monocode_core::search::fuzzy(query, field).is_some())
    }

    fn classic_session_status(
        &self,
        id: i64,
        session: &session_loading::Metadata<'_>,
        _cx: &Context<Self>,
    ) -> AnyElement {
        let status = div()
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_1()
            .text_size(px(11.))
            .line_height(px(16.5))
            .text_color(content(0.45));
        if session.needs_approval() {
            status
                .text_color(rgb(0xfbbf24))
                .child(icon("circle-alert", 12.))
                .child(if self.orchestration_summaries.contains_key(&id) {
                    "Needs input"
                } else {
                    "Need approval"
                })
                .into_any_element()
        } else if session.responding() {
            status
                .text_color(crate::theme::accent_color())
                .children(self.session_marks.get(&id).cloned())
                .child("Working...")
                .into_any_element()
        } else if self.unseen_finished.contains(&id) {
            status
                .text_color(rgb(0x34d399))
                .child(icon("check", 12.))
                .child("Done")
                .into_any_element()
        } else if session.has_saved_draft() {
            status
                .text_color(content(0.55))
                .child(icon("circle-dashed-hi", 12.))
                .child("Draft")
                .into_any_element()
        } else {
            status
                .child(monocode_core::time::relative(
                    session.summary.updated_at,
                    monocode_core::time::now_ms(),
                ))
                .into_any_element()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, VisualTestContext};

    fn bounds(cx: &mut VisualTestContext, selector: String) -> Option<gpui::Bounds<Pixels>> {
        cx.debug_bounds(Box::leak(selector.into_boxed_str()))
    }

    #[gpui::test]
    fn subagents_stay_off_the_sidebar_and_threads_are_rows(cx: &mut TestAppContext) {
        let (window, _, lead) = crate::shell::tests::window(cx);
        let cx = &mut VisualTestContext::from_window(window.into(), cx);
        let task = |session: Option<i64>, status: &str| monocode_core::orchestration::SummaryTask {
            session,
            title: "Write the tests".into(),
            provider: "codex".into(),
            model: None,
            status: status.into(),
        };
        let (thread, subagent, orchestrator, worker) = window
            .update(cx, |shell, _, cx| {
                let mut ids = Vec::new();
                for _ in 0..4 {
                    let summary = locked(&shell.store)
                        .unwrap()
                        .create("codex", &shell.workspace.project)
                        .unwrap();
                    ids.push(summary.id);
                    shell.add(summary, cx);
                }
                for id in ids.iter().copied().chain([lead]) {
                    shell.sessions[&id].update(cx, |s, _| s.summary.has_messages = true);
                }
                let (thread, subagent, orchestrator, worker) = (ids[0], ids[1], ids[2], ids[3]);
                // new_thread: only started by the lead.
                shell.started_by.insert(thread, lead);
                // thread_start: driven by it too.
                shell.started_by.insert(subagent, lead);
                shell.thread_parents.insert(subagent, lead);
                shell.orchestration_summaries.insert(
                    lead,
                    monocode_core::orchestration::Summary {
                        live: true,
                        tasks: vec![task(Some(subagent), "running")],
                    },
                );
                shell.orchestration_summaries.insert(
                    orchestrator,
                    monocode_core::orchestration::Summary {
                        live: false,
                        tasks: vec![task(Some(worker), "completed"), task(None, "pending")],
                    },
                );
                shell.selected_sessions.clear();
                shell.workspace.active = Some(lead);
                shell.invalidate_chrome(cx);
                (thread, subagent, orchestrator, worker)
            })
            .unwrap();
        cx.run_until_parked();
        let head = bounds(cx, format!("sidebar-session-{lead}")).expect("the lead is listed");
        let row = bounds(cx, format!("sidebar-session-{thread}")).expect("a thread is listed");
        assert_eq!(row.left(), head.left(), "an ordinary row, not inset");
        assert_eq!(row.size.height, px(48.), "with its \"from\" line");
        assert!(bounds(cx, format!("session-origin-{thread}")).is_some());
        for id in [subagent, worker] {
            assert!(
                bounds(cx, format!("sidebar-session-{id}")).is_none(),
                "a subagent is never a row of its own"
            );
        }
        for id in [lead, orchestrator] {
            assert!(
                bounds(cx, format!("rail-progress-{id}")).is_some(),
                "a lead sums up its subagents on its row"
            );
        }
        assert!(bounds(cx, format!("rail-agent-{orchestrator}-0")).is_none());
        window
            .read_with(cx, |shell, _| {
                assert_eq!(shell.subagent_lead(subagent), Some(lead));
                assert_eq!(shell.subagent_lead(worker), Some(orchestrator));
                assert_eq!(shell.subagent_lead(thread), None);
                assert_eq!(shell.subagents(lead), [subagent]);
            })
            .unwrap();
    }

    #[gpui::test]
    fn the_focused_leads_subagents_branch_off_its_card(cx: &mut TestAppContext) {
        let (window, _, lead) = crate::shell::tests::window(cx);
        let cx = &mut VisualTestContext::from_window(window.into(), cx);
        cx.update(|_, cx| cx.set_reduce_motion(true));
        let task = |session: Option<i64>, status: &str| monocode_core::orchestration::SummaryTask {
            session,
            title: "Write the tests".into(),
            provider: "codex".into(),
            model: None,
            status: status.into(),
        };
        let (other, subagent) = window
            .update(cx, |shell, _, cx| {
                let mut ids = Vec::new();
                for _ in 0..2 {
                    let summary = locked(&shell.store)
                        .unwrap()
                        .create("codex", &shell.workspace.project)
                        .unwrap();
                    ids.push(summary.id);
                    shell.add(summary, cx);
                }
                for id in ids.iter().copied().chain([lead]) {
                    shell.sessions[&id].update(cx, |s, _| s.summary.has_messages = true);
                }
                let (other, subagent) = (ids[0], ids[1]);
                shell.thread_parents.insert(subagent, lead);
                shell.orchestration_summaries.insert(
                    lead,
                    monocode_core::orchestration::Summary {
                        live: true,
                        tasks: vec![task(Some(subagent), "running"), task(None, "pending")],
                    },
                );
                shell.selected_sessions.clear();
                shell.workspace.active = Some(lead);
                shell.invalidate_chrome(cx);
                (other, subagent)
            })
            .unwrap();
        cx.run_until_parked();
        let card = bounds(cx, format!("sidebar-session-{lead}")).expect("the lead is listed");
        let tree = bounds(cx, format!("rail-tree-{lead}")).expect("focused, its tree is open");
        assert!(
            card.contains(&tree.origin),
            "the tree sits inside the lead's card"
        );
        assert_eq!(tree.size.height, px(TREE_TOP + 2. * TREE_ROW));
        let first = bounds(cx, format!("rail-subagent-{lead}-0")).unwrap();
        assert!(bounds(cx, format!("rail-subagent-{lead}-1")).is_some());

        // Its subagent opens beside it, and the tree stays while it's in front.
        cx.simulate_click(first.center(), gpui::Modifiers::none());
        cx.run_until_parked();
        window
            .read_with(cx, |shell, _| {
                assert_eq!(shell.workspace.active, Some(subagent));
                let tab = shell.workspace.tab_of(lead).unwrap();
                assert_eq!(shell.workspace.tab_of(subagent), Some(tab));
            })
            .unwrap();
        assert!(bounds(cx, format!("rail-tree-{lead}")).is_some());

        // Another chat in front folds it away.
        window
            .update(cx, |shell, window, cx| shell.open(other, window, cx))
            .unwrap();
        cx.run_until_parked();
        assert!(bounds(cx, format!("rail-tree-{lead}")).is_none());
        assert!(bounds(cx, format!("rail-progress-{lead}")).is_some());
    }
}
