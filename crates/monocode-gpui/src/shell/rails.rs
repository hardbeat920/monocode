//! Sidebar.tsx's rails: an orchestrator's agents (OrchestrationSidebarAgents,
//! `tree`) and the sessions an /operator thread started hang off a line under
//! its row. Each rail opens while its thread, or one of the thread's
//! sessions, is in view, chosen or working, and RevealHeight grows it in.
use super::*;
use crate::reveal::{Disclosure, bezier};
use std::time::{Duration, Instant};

/// RevealHeight's `reveal-height-in` and `reveal-height-out`.
const OPEN: Duration = Duration::from_millis(240);
const CLOSE: Duration = Duration::from_millis(180);

#[derive(Default)]
pub(crate) struct Rails {
    motions: HashMap<i64, Motion>,
    /// Agent rows opened to their details, by lead and task.
    open_tasks: HashSet<(i64, usize)>,
    /// This frame's /operator groups, and the sessions they took from the list.
    pub(super) started: HashMap<i64, Vec<i64>>,
    pub(super) children: HashSet<i64>,
    frames: Option<gpui::Task<()>>,
}
#[derive(Clone, Copy)]
struct Motion {
    open: bool,
    from: f32,
    start: Instant,
}
impl Motion {
    fn value(&self, now: Instant) -> f32 {
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
    fn settled(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.start) >= if self.open { OPEN } else { CLOSE }
    }
}

/// What hangs under a sidebar row.
pub(crate) enum Rail {
    /// Sessions this /operator thread started, newest first.
    Started(Vec<i64>),
    /// This orchestrator's agents.
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
    fn in_play(&self, id: i64, cx: &App) -> bool {
        (self.workspace.active == Some(id) && !self.mono_active(cx))
            || self.selected_sessions.contains(&id)
            || self
                .sessions
                .get(&id)
                .is_some_and(|s| s.read(cx).responding())
    }
    /// groupStartedSessions: each listed session another started joins the
    /// listed session at the top of its chain; one whose caller is not listed
    /// stays where it is.
    pub(super) fn started_groups(&self, ids: &[i64]) -> (HashMap<i64, Vec<i64>>, HashSet<i64>) {
        let listed: HashSet<i64> = ids.iter().copied().collect();
        let caller = |id: i64| {
            self.started_by
                .get(&id)
                .copied()
                .filter(|p| listed.contains(p))
        };
        let mut groups: HashMap<i64, Vec<i64>> = HashMap::new();
        let mut children = HashSet::new();
        for &id in ids {
            let Some(mut root) = caller(id) else {
                continue;
            };
            let mut seen = HashSet::from([id]);
            while let Some(up) = caller(root) {
                if !seen.insert(root) {
                    break;
                }
                root = up;
            }
            children.insert(id);
            groups.entry(root).or_default().push(id);
        }
        (groups, children)
    }
    /// Orchestrated workers live on their lead's rail, never in the list.
    pub(super) fn orchestration_worker(&self, id: i64) -> bool {
        self.orchestration_summaries
            .values()
            .any(|s| s.tasks.iter().any(|t| t.session == Some(id)))
    }
    fn rail(&self, id: i64, started: &HashMap<i64, Vec<i64>>) -> Option<Rail> {
        if let Some(summary) = self.orchestration_summaries.get(&id)
            && !summary.tasks.is_empty()
        {
            return Some(Rail::Agents(summary.clone()));
        }
        started
            .get(&id)
            .filter(|c| !c.is_empty())
            .map(|c| Rail::Started(c.clone()))
    }
    fn rail_open(&self, id: i64, rail: &Rail, cx: &App) -> bool {
        match rail {
            // orchestrationExpanded: the lead in view, chosen or working.
            Rail::Agents(_) => self.in_play(id, cx),
            Rail::Started(children) => {
                self.in_play(id, cx) || children.iter().any(|c| self.in_play(*c, cx))
            }
        }
    }
    /// Moves each rail toward its open state. A rail the sidebar first shows
    /// is simply there; later changes grow and fold.
    pub(super) fn sync_rails(&mut self, ids: &[i64], cx: &mut Context<Self>) {
        let (started, children) = if self.workspace.classic_layout {
            // Original Sidebar.tsx leaves /operator launches in the list.
            (HashMap::new(), HashSet::new())
        } else {
            self.started_groups(ids)
        };
        let now = Instant::now();
        let mut animating = false;
        let mut seen = HashSet::new();
        for &id in ids {
            let Some(rail) = self.rail(id, &started) else {
                continue;
            };
            seen.insert(id);
            let open = self.rail_open(id, &rail, cx);
            let reduced = cx.reduce_motion();
            match self.rails.motions.get(&id).copied() {
                None => {
                    self.rails.motions.insert(
                        id,
                        Motion {
                            open,
                            from: if open { 1. } else { 0. },
                            start: now - OPEN,
                        },
                    );
                }
                Some(motion) if motion.open != open => {
                    let from = if reduced {
                        if open { 1. } else { 0. }
                    } else {
                        motion.value(now)
                    };
                    self.rails.motions.insert(
                        id,
                        Motion {
                            open,
                            from,
                            start: if reduced { now - OPEN } else { now },
                        },
                    );
                }
                _ => {}
            }
            // A working agent's spinner keeps itself turning on the shared clock.
            animating |= !self.rails.motions[&id].settled(now);
        }
        self.rails.motions.retain(|id, _| seen.contains(id));
        self.rails.started = started;
        self.rails.children = children;
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
        let Some(rail) = self.rail(id, &self.rails.started) else {
            return if nested {
                self.session_row_with(id, true, None, cx)
            } else {
                self.session_row(id, cx)
            };
        };
        let now = Instant::now();
        let progress = self.rails.motions.get(&id).map_or(0., |m| m.value(now));
        let open = self.rails.motions.get(&id).is_some_and(|m| m.open);
        let row = self.session_row_with(id, nested, Some((&rail, open)), cx);
        if progress <= 0. {
            return row;
        }
        let body = match &rail {
            Rail::Started(children) => div()
                .pt_1()
                .flex()
                .flex_col()
                .children(children.iter().enumerate().map(|(ix, child)| {
                    step(
                        ix + 1 == children.len(),
                        self.session_row_with(*child, true, None, cx),
                    )
                }))
                .into_any_element(),
            Rail::Agents(summary) => self.agents(id, summary, cx),
        };
        // The rail drops from under the row's mark.
        let width =
            px((self.workspace.sidebar_width - 16. - 10. - if nested { 30. } else { 0. }).max(0.));
        let body = div().ml(px(10.)).child(body);
        div()
            .flex()
            .flex_col()
            .child(row)
            .child(if progress >= 1. {
                body.into_any_element()
            } else {
                div()
                    .ml(px(10.))
                    .opacity(progress)
                    .overflow_hidden()
                    .child(Disclosure::new(body.ml(px(0.)), width, progress))
                    .into_any_element()
            })
            .into_any_element()
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
        let rail = self
            .rail(id, &self.rails.started)
            .filter(|rail| matches!(rail, Rail::Agents(_)));
        let open = self
            .rails
            .motions
            .get(&id)
            .is_some_and(|motion| motion.open);
        self.classic_session_row_with(
            id,
            nested,
            compact,
            rail.as_ref().map(|rail| (rail, open)),
            cx,
        )
    }

    fn classic_session_row_with(
        &self,
        id: i64,
        nested: bool,
        compact: bool,
        rail: Option<(&Rail, bool)>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let Some(row) = self.rename_row(&session_groups::RenameTarget::Session(id), cx) {
            return row;
        }
        let work_item = self.work_item_badge(id, false, cx);
        let agents = match rail {
            Some((Rail::Agents(summary), true)) => Some(self.agents(id, summary, cx)),
            _ => None,
        };
        let session = self.session_metadata(id, cx);
        let selected = self.selected_sessions.contains(&id);
        let active =
            self.workspace.active == Some(id) && !self.mono_active(cx) && !self.app_view_open();
        let draft = session.has_saved_draft();
        let approval = session.needs_approval();
        let orchestration = matches!(rail, Some((Rail::Agents(_), true)));
        let header = !compact || orchestration;
        let started_in_view = match rail {
            Some((Rail::Started(children), _)) => children.iter().any(|child| {
                self.selected_sessions.contains(child)
                    || (self.workspace.active == Some(*child)
                        && !self.mono_active(cx)
                        && !self.app_view_open())
            }),
            _ => false,
        };
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
            .children(agents)
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
                            .when_some(rail, |d, (rail, _)| {
                                d.child(
                                    icon(
                                        match rail {
                                            Rail::Agents(_) => "share",
                                            Rail::Started(_) => "cursor-magic",
                                        },
                                        13.,
                                    )
                                    .text_color(content(0.5)),
                                )
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
            .pb(px(if orchestration {
                10.
            } else if compact {
                6.
            } else {
                8.
            }))
            .flex()
            .flex_col()
            .text_color(content(0.8))
            .when(selected, |d| d.bg(selected_fill))
            .when(!selected && approval, |d| d.bg(content(0.2)))
            .when(!selected && !approval && active, |d| d.bg(content(0.1)))
            .when(
                !selected && !approval && !active && (started_in_view || orchestration),
                |d| d.bg(content(0.05)),
            )
            .when(draft || approval, |d| {
                d.border_dashed()
                    .border_color(content(if active || selected || approval {
                        0.3
                    } else {
                        0.25
                    }))
            })
            .when(!selected && !approval && !active, |d| {
                d.hover(|s| s.bg(content(if orchestration { 0.1 } else { 0.05 })))
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
    /// OrchestrationSidebarAgents, `tree`.
    fn agents(
        &self,
        lead: i64,
        summary: &monocode_core::orchestration::Summary,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let classic = self.workspace.classic_layout;
        div()
            .when(!classic, |d| d.pt_1().pb_1())
            .when(classic, |d| {
                d.mt_1p5().child(
                    div()
                        .mb_0p5()
                        .px_0p5()
                        .flex()
                        .items_center()
                        .justify_between()
                        .text_size(px(11.))
                        .line_height(px(16.5))
                        .text_color(content(0.45))
                        .child(format!(
                            "{} {}",
                            summary.tasks.len(),
                            if summary.tasks.len() == 1 {
                                "agent"
                            } else {
                                "agents"
                            }
                        ))
                        .child(format!(
                            "{}/{} done",
                            summary
                                .tasks
                                .iter()
                                .filter(|task| task.status == "completed")
                                .count(),
                            summary.tasks.len()
                        )),
                )
            })
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .when(classic, |d| d.mx(px(-8.)).gap_px())
                    .children(summary.tasks.iter().enumerate().map(|(ix, task)| {
                        let needs_input = task.session.is_some_and(|s| {
                            self.sessions
                                .get(&s)
                                .is_some_and(|v| v.read(cx).needs_approval())
                        });
                        let open = self.rails.open_tasks.contains(&(lead, ix)) || needs_input;
                        let label = task.label(summary, needs_input);
                        let working = summary.live && task.status == "running" && !needs_input;
                        let alert =
                            needs_input || matches!(task.status.as_str(), "failed" | "blocked");
                        let done = task.status == "completed";
                        let tone: gpui::Hsla = if alert {
                            rgb(0xfbbf24).into()
                        } else if working {
                            accent_color()
                        } else if done {
                            rgb(0x34d399).into()
                        } else {
                            content(0.45)
                        };
                        let model = crate::pickers::model_name(
                            Kind::from_id(&task.provider),
                            task.model.as_deref(),
                        )
                        .to_string();
                        let group: SharedString = format!("rail-agent-{lead}-{ix}").into();
                        let header = div()
                            .id(("rail-agent", (lead as usize) << 8 | ix))
                            .debug_selector(move || format!("rail-agent-{lead}-{ix}"))
                            .role(Role::Button)
                            .aria_label(format!("Agent details: {}", task.title))
                            .aria_expanded(open)
                            .group(group.clone())
                            .h(px(if classic { 28.5 } else { 28. }))
                            .w_full()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .px(px(if classic { 8. } else { 6. }))
                            .rounded_md()
                            .cursor_pointer()
                            .when(open && !classic, |d| d.bg(content(0.1)))
                            .when(!open, |d| d.hover(|s| s.bg(content(0.1))))
                            .child(
                                div()
                                    .size(px(14.))
                                    .flex_shrink_0()
                                    .relative()
                                    .opacity(0.75)
                                    .child(
                                        div()
                                            .when(classic && open, |d| d.invisible())
                                            .when(classic, |d| {
                                                d.group_hover(group.clone(), |s| s.invisible())
                                            })
                                            .child(provider_icon(&task.provider, 14.)),
                                    )
                                    .when(classic, |d| {
                                        d.child(
                                            div()
                                                .absolute()
                                                .top_0()
                                                .left_0()
                                                .size_full()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .invisible()
                                                .when(open, |d| d.visible())
                                                .group_hover(group.clone(), |s| s.visible())
                                                .child(
                                                    icon(
                                                        if open {
                                                            "chevron-down"
                                                        } else {
                                                            "chevron-right"
                                                        },
                                                        12.,
                                                    )
                                                    .text_color(content(0.45)),
                                                ),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .truncate()
                                    .text_size(px(if classic { 12. } else { 13. }))
                                    .when(classic, |d| d.line_height(px(16.5)))
                                    .text_color(content(if classic {
                                        0.8
                                    } else if open {
                                        1.
                                    } else {
                                        0.55
                                    }))
                                    .when(!classic && !open, |d| {
                                        d.group_hover(group.clone(), |s| s.text_color(content(0.8)))
                                    })
                                    .child(task.title.clone()),
                            )
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .text_size(px(11.))
                                    .when(classic, |d| d.line_height(px(16.5)))
                                    .text_color(tone)
                                    .when(alert, |d| {
                                        d.child(icon("circle-alert", 12.).text_color(tone))
                                    })
                                    .when(working, |d| {
                                        d.child(crate::pulse::looping(
                                            SharedString::from(format!("rail-spinner-{lead}-{ix}")),
                                            crate::session_mark::SPINNER_PERIOD,
                                            crate::pulse::Rate::Half,
                                            crate::session_mark::spinner_at,
                                        ))
                                    })
                                    .when(done && !alert, |d| {
                                        d.child(icon("check", 12.).text_color(tone))
                                    })
                                    // On the rail the spinner says it on its own.
                                    .when(classic || !working, |d| d.child(label)),
                            )
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !this.rails.open_tasks.remove(&(lead, ix)) {
                                    this.rails.open_tasks.insert((lead, ix));
                                }
                                this.notify_sidebar(cx);
                            }));
                        let worker = task.session;
                        let running = worker.is_some_and(|s| {
                            summary.live
                                && self
                                    .sessions
                                    .get(&s)
                                    .is_some_and(|v| v.read(cx).responding())
                        });
                        let solid = |id: &'static str, label: &'static str| {
                            div()
                                .id((id, (lead as usize) << 8 | ix))
                                .role(Role::Button)
                                .aria_label(label)
                                .rounded(px(4.))
                                .bg(content(0.15))
                                .px(px(6.))
                                .py(px(2.))
                                .text_size(px(11.))
                                .text_color(content(0.75))
                                .cursor_pointer()
                                .hover(|s| s.bg(content(0.25)).text_color(content(1.)))
                                .child(label)
                        };
                        let details = div()
                            .pl(px(if classic { 26. } else { 6. }))
                            .pt(px(if classic { 0. } else { 6. }))
                            .pb_3()
                            .pr_2()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .min_w_0()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .text_size(px(11.))
                                    .text_color(content(0.45))
                                    .child(provider_icon(&task.provider, 14.))
                                    .child(div().min_w_0().truncate().child(model)),
                            )
                            .when(worker.is_some() || running, |d| {
                                d.child(
                                    div()
                                        .flex()
                                        .flex_wrap()
                                        .items_center()
                                        .gap_1()
                                        .when_some(worker, |d, worker| {
                                            d.child(
                                                solid("rail-agent-open", "See details").on_click(
                                                    cx.listener(move |this, _, window, cx| {
                                                        this.open(worker, window, cx)
                                                    }),
                                                ),
                                            )
                                        })
                                        .when_some(worker.filter(|_| running), |d, worker| {
                                            d.child(
                                                solid("rail-agent-cancel", "Cancel task").on_click(
                                                    cx.listener(move |this, _, _, cx| {
                                                        if let Some(view) =
                                                            this.sessions.get(&worker)
                                                        {
                                                            view.update(cx, |v, cx| {
                                                                v.stop_if_busy(cx)
                                                            });
                                                        }
                                                    }),
                                                ),
                                            )
                                        }),
                                )
                            });
                        let row = div()
                            .flex()
                            .flex_col()
                            .when(classic, |d| {
                                d.rounded_md().when(open, |d| d.bg(content(0.1)))
                            })
                            .child(header)
                            .when(open, |d| d.child(details));
                        if classic {
                            row.into_any_element()
                        } else {
                            step(ix + 1 == summary.tasks.len(), row).into_any_element()
                        }
                    })),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, VisualTestContext};

    fn bounds(cx: &mut VisualTestContext, selector: String) -> Option<gpui::Bounds<Pixels>> {
        cx.debug_bounds(Box::leak(selector.into_boxed_str()))
    }
    /// Rail motion runs on the wall clock, as the window's frames do.
    fn settle(window: gpui::WindowHandle<Shell>, cx: &mut VisualTestContext) {
        for _ in 0..2 {
            std::thread::sleep(OPEN + Duration::from_millis(20));
            window
                .update(cx, |shell, _, cx| {
                    shell.invalidate_chrome(cx);
                    cx.notify();
                })
                .unwrap();
            cx.run_until_parked();
        }
    }

    #[gpui::test]
    fn operator_and_orchestrator_threads_hang_their_work_on_a_rail_that_follows_selection(
        cx: &mut TestAppContext,
    ) {
        let (window, _, lead) = crate::shell::tests::window(cx);
        let cx = &mut VisualTestContext::from_window(window.into(), cx);
        let (child, other, worker) = window
            .update(cx, |shell, _, cx| {
                let mut ids = Vec::new();
                for _ in 0..3 {
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
                let (child, other, worker) = (ids[0], ids[1], ids[2]);
                shell.started_by.insert(child, lead);
                shell.orchestration_summaries.insert(
                    other,
                    monocode_core::orchestration::Summary {
                        live: false,
                        tasks: vec![monocode_core::orchestration::SummaryTask {
                            session: Some(worker),
                            title: "Write the tests".into(),
                            provider: "codex".into(),
                            model: None,
                            status: "completed".into(),
                        }],
                    },
                );
                shell.selected_sessions.clear();
                shell.workspace.active = Some(lead);
                shell.invalidate_chrome(cx);
                (child, other, worker)
            })
            .unwrap();
        settle(window, cx);
        let head = bounds(cx, format!("sidebar-session-{lead}")).expect("the thread is listed");
        let nested =
            bounds(cx, format!("sidebar-session-{child}")).expect("its session hangs below");
        assert!(nested.top() >= head.bottom(), "under its caller");
        assert!(nested.left() > head.left(), "on the rail, inset");
        assert_eq!(nested.size.height, px(28.));
        assert!(
            bounds(cx, format!("sidebar-session-{worker}")).is_none(),
            "a worker is never listed on its own"
        );
        assert!(
            bounds(cx, format!("rail-agent-{other}-0")).is_none(),
            "an orchestrator's rail is closed while it is out of play"
        );
        // Clicking the orchestrator opens its agents and folds the other rail;
        // the click does not also land on an agent that grows in under it.
        let row = bounds(cx, format!("sidebar-session-{other}")).unwrap();
        let clicked = Instant::now();
        cx.simulate_click(row.center(), Default::default());
        cx.run_until_parked();
        window
            .read_with(cx, |shell, _| {
                assert_eq!(shell.workspace.active, Some(other));
                assert!(shell.rails.open_tasks.is_empty());
            })
            .unwrap();
        let opening = window
            .read_with(cx, |shell, _| shell.rails.motions[&other])
            .unwrap();
        // A motion that starts at the click, rather than one already settled.
        assert!(
            opening.open && opening.start >= clicked,
            "the rail grows in"
        );
        assert!(opening.value(opening.start) < 0.01);
        settle(window, cx);
        let agent = bounds(cx, format!("rail-agent-{other}-0")).expect("its agents show");
        window
            .read_with(cx, |shell, _| assert!(shell.rails.open_tasks.is_empty()))
            .unwrap();
        cx.simulate_click(agent.center(), Default::default());
        cx.run_until_parked();
        window
            .read_with(cx, |shell, _| {
                assert!(
                    shell.rails.open_tasks.contains(&(other, 0)),
                    "a click opens its details"
                )
            })
            .unwrap();
        settle(window, cx);
        let agent = bounds(cx, format!("rail-agent-{other}-0")).unwrap();
        cx.simulate_click(agent.center(), Default::default());
        cx.run_until_parked();
        window
            .read_with(cx, |shell, _| {
                assert!(shell.rails.open_tasks.is_empty(), "and closes them")
            })
            .unwrap();
        assert!(
            bounds(cx, format!("sidebar-session-{child}")).is_none(),
            "the operator rail folds away once its thread leaves play"
        );
    }
}
