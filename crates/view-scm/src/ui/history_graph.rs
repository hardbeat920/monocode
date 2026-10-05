//! Port of src/features/source-control/ui/GitHistoryGraph.tsx: the Graph
//! section under the changes list. Each row draws its slice of the graph
//! with GPUI paths and quads, from the SVG paths `git_graph` computes.

use std::ops::Range;

use gpui::{
    Bounds, Context, EventEmitter, Hsla, InteractiveElement as _, IntoElement, ParentElement as _,
    PathBuilder, Pixels, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, Task, UniformListScrollHandle, Window, canvas, div, point,
    prelude::FluentBuilder as _, px, size, uniform_list,
};
use monocode_ui::widgets::tooltip;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use crate::git::GitHistoryCommit;
use crate::model::git_graph::{
    GRAPH_ROW_PX, GraphCircle, GraphCommit, GraphRef, HistoryItemGraph, HistoryItemViewModel,
    ItemKind, PathCommand, history_item_graph, layout_git_graph, parse_path,
};
use crate::scm::{Scm, ScmEvent};
use crate::ui::common::{hex_color, mix};

pub const GRAPH_PANEL_MIN: f32 = 120.;
pub const GRAPH_PANEL_DEFAULT: f32 = 240.;

#[derive(Clone, Debug, PartialEq)]
pub enum GraphEvent {
    /// The header was clicked; the owner flips `expanded`.
    ToggleExpanded,
    OpenCommit {
        commit: GitHistoryCommit,
        pin: bool,
    },
}

pub struct GitHistoryGraph {
    scm: Scm,
    cwd: String,
    enabled: bool,
    expanded: bool,
    selected_sha: Option<String>,
    commits: Vec<GitHistoryCommit>,
    rows: Vec<HistoryItemViewModel>,
    graphs: Vec<HistoryItemGraph>,
    hovered: Option<usize>,
    scroll: UniformListScrollHandle,
    load: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<GraphEvent> for GitHistoryGraph {}

impl GitHistoryGraph {
    pub fn new(
        scm: Scm,
        cwd: impl Into<String>,
        enabled: bool,
        expanded: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscriptions = vec![
            cx.subscribe(&scm.state, |this, _, event: &ScmEvent, cx| match event {
                ScmEvent::GitChanged => this.load(cx),
            }),
            cx.observe_window_activation(window, |this, window, cx| {
                if window.is_window_active() {
                    this.load(cx);
                }
            }),
        ];
        let mut this = Self {
            scm,
            cwd: cwd.into(),
            enabled,
            expanded,
            selected_sha: None,
            commits: Vec::new(),
            rows: Vec::new(),
            graphs: Vec::new(),
            hovered: None,
            scroll: UniformListScrollHandle::new(),
            load: None,
            _subscriptions: subscriptions,
        };
        this.activate(cx);
        this
    }

    fn active(&self) -> bool {
        self.enabled && self.expanded && !self.cwd.is_empty() && self.cwd != "~"
    }

    pub fn commits(&self) -> &[GitHistoryCommit] {
        &self.commits
    }

    pub fn expanded(&self) -> bool {
        self.expanded
    }

    pub fn set_expanded(&mut self, expanded: bool, cx: &mut Context<Self>) {
        if self.expanded != expanded {
            self.expanded = expanded;
            self.activate(cx);
        }
    }

    pub fn set_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.enabled != enabled {
            self.enabled = enabled;
            self.activate(cx);
        }
    }

    pub fn set_selected_sha(&mut self, sha: Option<String>, cx: &mut Context<Self>) {
        if self.selected_sha != sha {
            self.selected_sha = sha;
            cx.notify();
        }
    }

    /// The effect that ran when `cwd` or `enabled` changed: show the cached
    /// history, then revalidate.
    fn activate(&mut self, cx: &mut Context<Self>) {
        if !self.active() {
            self.set_commits(Vec::new(), cx);
            return;
        }
        let cached = self
            .scm
            .state
            .read(cx)
            .history_by_cwd
            .get(&self.cwd)
            .cloned()
            .unwrap_or_default();
        self.set_commits(cached, cx);
        self.load(cx);
    }

    /// `load` from `useGitHistory`.
    pub fn load(&mut self, cx: &mut Context<Self>) {
        if !self.active() {
            return;
        }
        let cwd = self.cwd.clone();
        let read = {
            let cwd = cwd.clone();
            self.scm.run(cx, move |git| git.git_history(&cwd))
        };
        self.load = Some(cx.spawn(async move |this, cx| {
            let result = read.await;
            let _ = this.update(cx, |this, cx| {
                if this.cwd != cwd {
                    return;
                }
                match result {
                    Ok(next) => {
                        if same_history(&this.commits, &next.commits) {
                            return;
                        }
                        this.scm.state.update(cx, |state, _| {
                            state
                                .history_by_cwd
                                .insert(cwd.clone(), next.commits.clone());
                        });
                        this.set_commits(next.commits, cx);
                    }
                    Err(_) => {
                        this.scm.state.update(cx, |state, _| {
                            state.history_by_cwd.remove(&cwd);
                        });
                        this.set_commits(Vec::new(), cx);
                    }
                }
            });
        }));
    }

    fn set_commits(&mut self, commits: Vec<GitHistoryCommit>, cx: &mut Context<Self>) {
        let graph_commits: Vec<GraphCommit> = commits.iter().map(GraphCommit::from).collect();
        self.rows = layout_git_graph(&graph_commits);
        self.graphs = self.rows.iter().map(history_item_graph).collect();
        self.commits = commits;
        cx.notify();
    }

    fn render_rows(
        &mut self,
        range: Range<usize>,
        cx: &mut Context<Self>,
    ) -> Vec<gpui::AnyElement> {
        let theme = Theme::of(cx).clone();
        range
            .filter_map(|index| {
                let commit = self.commits.get(index)?;
                let row = self.rows.get(index)?;
                let graph = self.graphs.get(index)?.clone();
                let active = self.selected_sha.as_deref() == Some(commit.sha.as_str());
                let hovered = self.hovered == Some(index);
                Some(
                    history_row(index, commit, row, graph, active, hovered, &theme, cx)
                        .into_any_element(),
                )
            })
            .collect()
    }
}

#[allow(clippy::too_many_arguments)]
fn history_row(
    index: usize,
    commit: &GitHistoryCommit,
    row: &HistoryItemViewModel,
    graph: HistoryItemGraph,
    active: bool,
    hovered: bool,
    theme: &Theme,
    cx: &mut Context<GitHistoryGraph>,
) -> impl IntoElement {
    let c = theme.colors;
    let head = row.kind == ItemKind::Head;
    let badge = row
        .refs
        .iter()
        .find(|r| r.color.is_some())
        .or_else(|| row.refs.first())
        .cloned();
    let title = if commit.author.is_empty() {
        format!("{} {}", commit.short_sha, commit.subject)
    } else {
        format!(
            "{} {} — {}",
            commit.short_sha, commit.subject, commit.author
        )
    };
    let subject: SharedString = if commit.subject.is_empty() {
        commit.short_sha.clone().into()
    } else {
        commit.subject.clone().into()
    };
    let background = c.background_base;
    let content = c.content;
    let width = graph.width;
    let open_commit = commit.clone();
    let pin_commit = commit.clone();
    div()
        .id(("history-row", index))
        .flex()
        .h(u(GRAPH_ROW_PX))
        .w_full()
        .min_w_0()
        .items_stretch()
        .pr(u(8.))
        .text_color(content)
        .when(active, |el| el.bg(c.selection))
        .when(!active, |el| el.hover(|s| s.bg(theme.content(0.05))))
        .tooltip(tooltip(title))
        .on_hover(cx.listener(move |this, hovering: &bool, _, cx| {
            let next = if *hovering {
                Some(index)
            } else if this.hovered == Some(index) {
                None
            } else {
                this.hovered
            };
            if next != this.hovered {
                this.hovered = next;
                cx.notify();
            }
        }))
        .on_click(cx.listener(move |_, event: &gpui::ClickEvent, _, cx| {
            if event.click_count() >= 2 {
                cx.emit(GraphEvent::OpenCommit {
                    commit: pin_commit.clone(),
                    pin: true,
                });
            } else {
                cx.emit(GraphEvent::OpenCommit {
                    commit: open_commit.clone(),
                    pin: false,
                });
            }
        }))
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    paint_graph(
                        &graph, bounds, background, content, hovered, active, head, window,
                    );
                },
            )
            .flex_none()
            .w(u(width))
            .h(u(GRAPH_ROW_PX)),
        )
        .child(
            div()
                .ml(u(4.))
                .flex()
                .flex_1()
                .min_w_0()
                .items_center()
                .overflow_hidden()
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_px(12.)
                        .line_height(u(22.))
                        .when(head, |el| el.semibold())
                        .child(subject),
                )
                .when(!commit.author.is_empty(), |el| {
                    el.child(
                        div()
                            .ml(u(8.))
                            .min_w_0()
                            .flex_shrink(1.)
                            .truncate()
                            .text_px(12.)
                            .line_height(u(22.))
                            .text_color(theme.content(0.45))
                            .child(commit.author.clone()),
                    )
                }),
        )
        .when_some(badge, |el, badge| el.child(ref_pill(&badge, theme)))
}

/// `RefPill`.
fn ref_pill(r: &GraphRef, theme: &Theme) -> impl IntoElement {
    let local = r.kind == "local";
    let (bg, ink) = match r.color {
        Some(color) => (hex_color(color), theme.colors.background_base),
        None => (theme.content(0.10), theme.content(0.55)),
    };
    div()
        .ml(u(4.))
        .flex()
        .flex_none()
        .h(u(14.))
        .min_w_0()
        .max_w(u(104.))
        .self_center()
        .items_center()
        .gap(u(2.))
        .overflow_hidden()
        .rounded_full()
        .px(u(6.))
        .text_px(10.)
        .line_height(u(10.))
        .bg(bg)
        .text_color(ink)
        .when(local, |el| {
            el.child(icon(IconName::GitBranch).size(u(10.)).text_color(ink))
        })
        .child(div().min_w_0().truncate().child(r.name.clone()))
}

/// Draw one row's paths and circles in `bounds`, with the CSS rules for the
/// circle strokes and the HEAD cutout.
#[allow(clippy::too_many_arguments)]
fn paint_graph(
    graph: &HistoryItemGraph,
    bounds: Bounds<Pixels>,
    background: Hsla,
    content: Hsla,
    hovered: bool,
    selected: bool,
    head: bool,
    window: &mut Window,
) {
    let scale: f32 = f32::from(window.rem_size()) / 16.;
    let origin = bounds.origin;
    let at = |x: f32, y: f32| point(origin.x + px(x * scale), origin.y + px(y * scale));
    for path in &graph.paths {
        let mut builder = PathBuilder::stroke(px(path.stroke_width * scale));
        for command in parse_path(&path.d) {
            match command {
                PathCommand::MoveTo(x, y) => builder.move_to(at(x, y)),
                PathCommand::LineTo(x, y) => builder.line_to(at(x, y)),
                PathCommand::ArcTo {
                    rx,
                    ry,
                    large_arc,
                    sweep,
                    x,
                    y,
                } => builder.arc_to(
                    point(px(rx * scale), px(ry * scale)),
                    px(0.),
                    large_arc,
                    sweep,
                    at(x, y),
                ),
            }
        }
        if let Ok(built) = builder.build() {
            window.paint_path(built, hex_color(path.color));
        }
    }
    let hover_mix = mix(content, background, 0.05);
    let selected_mix = mix(content, background, 0.10);
    let count = graph.circles.len();
    for (i, circle) in graph.circles.iter().enumerate() {
        let first = i == 0;
        let second = i == 1;
        let last = i + 1 == count;
        // `.git-history-item svg circle { stroke: background-base }` and the
        // hover and selection overrides after it.
        let mut stroke = Some(background);
        if hovered && first {
            stroke = None;
        }
        if hovered && second {
            stroke = Some(hover_mix);
        }
        if selected && second {
            stroke = Some(selected_mix);
        }
        let mut fill = circle.fill.map(hex_color);
        if head && last {
            fill = Some(if selected {
                selected_mix
            } else if hovered {
                hover_mix
            } else {
                background
            });
        }
        paint_circle(circle, fill, stroke, scale, origin, window);
    }
}

fn paint_circle(
    circle: &GraphCircle,
    fill: Option<Hsla>,
    stroke: Option<Hsla>,
    scale: f32,
    origin: gpui::Point<Pixels>,
    window: &mut Window,
) {
    let center = point(
        origin.x + px(circle.cx * scale),
        origin.y + px(circle.cy * scale),
    );
    let disc = |radius: f32| {
        let r = px(radius * scale);
        Bounds::new(point(center.x - r, center.y - r), size(r * 2., r * 2.))
    };
    let half = circle.stroke_width / 2.;
    match (fill, stroke) {
        (Some(fill), Some(stroke)) => {
            let outer = circle.r + half;
            window.paint_quad(gpui::fill(disc(outer), stroke).corner_radii(px(outer * scale)));
            let inner = circle.r - half;
            if inner > 0. {
                window.paint_quad(gpui::fill(disc(inner), fill).corner_radii(px(inner * scale)));
            }
        }
        (Some(fill), None) => {
            window.paint_quad(gpui::fill(disc(circle.r), fill).corner_radii(px(circle.r * scale)));
        }
        (None, Some(stroke)) => {
            let outer = circle.r + half;
            window.paint_quad(gpui::quad(
                disc(outer),
                px(outer * scale),
                gpui::transparent_black(),
                px(circle.stroke_width * scale),
                stroke,
                gpui::BorderStyle::Solid,
            ));
        }
        (None, None) => {}
    }
}

/// `sameHistory`.
pub fn same_history(prev: &[GitHistoryCommit], next: &[GitHistoryCommit]) -> bool {
    prev.len() == next.len()
        && prev.iter().zip(next).all(|(a, b)| {
            a.sha == b.sha
                && a.subject == b.subject
                && a.head == b.head
                && a.refs.len() == b.refs.len()
                && a.refs
                    .iter()
                    .zip(&b.refs)
                    .all(|(x, y)| x.name == y.name && x.kind == y.kind)
        })
}

impl Render for GitHistoryGraph {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let expanded = self.expanded;
        let header = div()
            .id("graph-header")
            .flex()
            .w_full()
            .flex_none()
            .items_center()
            .gap(u(4.))
            .px(u(12.))
            .when(expanded, |el| el.h(u(28.)))
            .when(!expanded, |el| el.h_full())
            .hover(|s| s.bg(theme.content(0.05)))
            .on_click(cx.listener(|_, _, _, cx| cx.emit(GraphEvent::ToggleExpanded)))
            .child(
                div()
                    .text_px(10.)
                    .semibold()
                    .text_color(theme.content(0.55))
                    .child("GRAPH"),
            )
            .child(div().flex_1())
            .child(
                icon(if expanded {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .size(u(14.))
                .text_color(theme.content(0.50)),
            );
        let mut root = div()
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .min_w_0()
            .overflow_hidden()
            .child(header);
        if !expanded {
            return root;
        }
        let body = if self.cwd.is_empty() || self.cwd == "~" {
            empty_line("No project folder", &theme).into_any_element()
        } else if self.commits.is_empty() {
            empty_line("No commits yet", &theme).into_any_element()
        } else {
            uniform_list(
                "history-rows",
                self.commits.len(),
                cx.processor(|this, range: Range<usize>, _, cx| this.render_rows(range, cx)),
            )
            .track_scroll(&self.scroll)
            .flex_1()
            .min_h_0()
            .into_any_element()
        };
        root = root.child(body);
        root
    }
}

fn empty_line(text: &'static str, theme: &Theme) -> impl IntoElement {
    div()
        .px(u(12.))
        .py(u(8.))
        .text_px(12.)
        .text_color(theme.content(0.45))
        .child(text)
}

/// Keeps `height` within the graph's limits (`clamp` in GraphResizeSash).
pub fn clamp_graph_height(value: f32, max: f32) -> f32 {
    value.round().max(GRAPH_PANEL_MIN).min(max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::GitHistoryRef;

    fn commit(sha: &str, subject: &str) -> GitHistoryCommit {
        GitHistoryCommit {
            sha: sha.into(),
            short_sha: sha.into(),
            parents: Vec::new(),
            author: "me".into(),
            timestamp: 0,
            subject: subject.into(),
            refs: vec![GitHistoryRef {
                name: "main".into(),
                kind: "local".into(),
            }],
            head: false,
        }
    }

    #[test]
    fn same_history_compares_shas_subjects_and_refs() {
        let a = vec![commit("a", "one")];
        assert!(same_history(&a, &[commit("a", "one")]));
        assert!(!same_history(&a, &[commit("a", "two")]));
        let mut moved = commit("a", "one");
        moved.refs[0].name = "feature".into();
        assert!(!same_history(&a, &[moved]));
        assert_eq!(clamp_graph_height(50., 300.), GRAPH_PANEL_MIN);
        assert_eq!(clamp_graph_height(500., 300.), 300.);
    }
}
