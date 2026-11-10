//! GitHistoryGraph.tsx's standalone History tab: its toolbar, search and
//! 44px rows with the swimlane graph painted per row.
use crate::{
    git_graph::{self, GraphCommit, NodeKind, Row, RowGraph, SWIMLANE_WIDTH},
    git_status::{GitChanged, ProjectGit},
    input::{self, Input},
    right_panel::*,
    theme::*,
};
use gpui::{
    App, Bounds, Context, Entity, FocusHandle, Focusable, FontWeight, Hsla, KeyBinding, Role,
    ScrollStrategy, SharedString, Subscription, UniformListScrollHandle, Window, actions, canvas,
    div, fill, point, prelude::*, px, rgb, size, uniform_list,
};
use monocode_core::scm::{self, HistoryCommit};
use std::{collections::HashMap, path::PathBuf};

const ROW: f32 = 44.;
actions!(history, [CloseSearch]);

pub(crate) fn bind(cx: &mut App) {
    cx.bind_keys([KeyBinding::new(
        "escape",
        CloseSearch,
        Some("HistorySearch"),
    )]);
}

pub(crate) enum HistoryEvent {
    OpenCommit {
        cwd: String,
        sha: String,
        short_sha: String,
        subject: String,
    },
}
impl gpui::EventEmitter<HistoryEvent> for History {}

struct Item {
    commit: HistoryCommit,
    row: Row,
    graph: RowGraph,
}

pub(crate) struct History {
    git: Entity<ProjectGit>,
    cwd: String,
    enabled: bool,
    items: Vec<Item>,
    /// historyByCwd: a revisit opens on the last read.
    cache: HashMap<String, Vec<HistoryCommit>>,
    request: u64,
    loading: bool,
    error: Option<String>,
    search_open: bool,
    search: Entity<Input>,
    query: String,
    visible: Vec<usize>,
    selected: Option<String>,
    hovered: Option<usize>,
    scroll: UniformListScrollHandle,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl History {
    pub(crate) fn new(git: Entity<ProjectGit>, _: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| {
            let mut input = Input::field("Search commits", "Search history…", content(0.35), cx);
            input.set_font_size(12.);
            input
        });
        let subscriptions = vec![
            cx.subscribe(&search, |this, search, _: &input::Changed, cx| {
                this.query = search.read(cx).content.to_string();
                this.filter();
                cx.notify();
            }),
            cx.subscribe(&git, |this, _, _: &GitChanged, cx| this.git_changed(cx)),
        ];
        Self {
            git,
            cwd: String::new(),
            enabled: false,
            items: Vec::new(),
            cache: HashMap::new(),
            request: 0,
            loading: false,
            error: None,
            search_open: false,
            search,
            query: String::new(),
            visible: Vec::new(),
            selected: None,
            hovered: None,
            scroll: UniformListScrollHandle::new(),
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    }
    pub(crate) fn set_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        let was = self.enabled;
        self.enabled = enabled;
        if enabled && !was {
            self.sync_project(cx);
            self.load(cx);
        }
    }
    /// The window came back into focus.
    pub(crate) fn resume(&mut self, cx: &mut Context<Self>) {
        if self.enabled {
            self.load(cx);
        }
    }
    fn git_changed(&mut self, cx: &mut Context<Self>) {
        if !self.enabled {
            return;
        }
        self.sync_project(cx);
        self.load(cx);
    }
    /// A new checkout clears the search and opens on its cached history.
    fn sync_project(&mut self, cx: &mut Context<Self>) -> bool {
        let root = self.git.read(cx).root.clone();
        if root == self.cwd {
            return false;
        }
        self.cwd = root;
        self.request += 1;
        self.loading = false;
        self.error = None;
        self.search_open = false;
        self.query.clear();
        self.search
            .update(cx, |s, cx| s.set_text(String::new(), cx));
        let cached = self.cache.get(&self.cwd).cloned().unwrap_or_default();
        self.set_commits(cached);
        cx.notify();
        true
    }
    fn available(&self) -> bool {
        !self.cwd.is_empty() && self.cwd != "~"
    }
    fn load(&mut self, cx: &mut Context<Self>) {
        if !self.enabled || !self.available() {
            return;
        }
        self.request += 1;
        let request = self.request;
        self.loading = true;
        self.error = None;
        let cwd = self.cwd.clone();
        let path = PathBuf::from(&cwd);
        let work = cx
            .background_executor()
            .spawn(async move { scm::history(&path, Some(200)) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                if this.request != request {
                    return;
                }
                this.loading = false;
                match result {
                    Ok(history) => {
                        let same = this.items.len() == history.commits.len()
                            && this
                                .items
                                .iter()
                                .zip(&history.commits)
                                .all(|(a, b)| &a.commit == b);
                        if !same {
                            this.cache.insert(cwd, history.commits.clone());
                            this.set_commits(history.commits);
                        }
                    }
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn set_commits(&mut self, commits: Vec<HistoryCommit>) {
        let graph: Vec<GraphCommit> = commits
            .iter()
            .map(|c| GraphCommit {
                sha: c.sha.clone(),
                parents: c.parents.clone(),
                head: c.head,
                refs: c
                    .refs
                    .iter()
                    .map(|r| (r.name.clone(), r.kind.clone()))
                    .collect(),
            })
            .collect();
        let rows = git_graph::layout(&graph);
        self.items = commits
            .into_iter()
            .zip(rows)
            .map(|(commit, row)| {
                let graph = git_graph::row_graph(&row, ROW);
                Item { commit, row, graph }
            })
            .collect();
        self.filter();
    }
    fn filter(&mut self) {
        let needle = self.query.trim().to_lowercase();
        self.visible = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                needle.is_empty() || {
                    let c = &item.commit;
                    [&c.subject, &c.author, &c.sha]
                        .into_iter()
                        .chain(c.refs.iter().map(|r| &r.name))
                        .any(|v| v.to_lowercase().contains(&needle))
                }
            })
            .map(|(ix, _)| ix)
            .collect();
        self.hovered = None;
    }
    fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_open = true;
        let focus = self.search.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        cx.notify();
    }
    fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_open = false;
        self.query.clear();
        self.search
            .update(cx, |s, cx| s.set_text(String::new(), cx));
        self.filter();
        window.focus(&self.focus, cx);
        cx.notify();
    }
    fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        let commit = &self.items[ix].commit;
        self.selected = Some(commit.sha.clone());
        cx.emit(HistoryEvent::OpenCommit {
            cwd: self.cwd.clone(),
            sha: commit.sha.clone(),
            short_sha: commit.short_sha.clone(),
            subject: commit.subject.clone(),
        });
        cx.notify();
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let head = self.items.iter().find(|i| i.commit.head);
        let branch = head.and_then(|h| {
            h.commit
                .refs
                .iter()
                .find(|r| r.kind == "local")
                .map(|r| r.name.clone())
        });
        let label: SharedString = branch
            .clone()
            .or_else(|| head.map(|h| h.commit.short_sha.clone()))
            .unwrap_or_else(|| "History".into())
            .into();
        let can_refresh = self.available() && !self.loading;
        div()
            .flex_shrink_0()
            .child(pane_header("history-toolbar", "History").map(|d| {
                if self.search_open {
                    d.key_context("HistorySearch")
                        .on_action(cx.listener(|this, _: &CloseSearch, window, cx| {
                            this.close_search(window, cx)
                        }))
                        .child(icon("search", 14.).text_color(content(0.45)))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .font_weight(FontWeight::NORMAL)
                                .text_color(content(1.))
                                .child(self.search.clone()),
                        )
                        .child(
                            header_action(
                                "history-close-search",
                                "Close history search",
                                "x",
                                12.,
                                true,
                                false,
                            )
                            .on_click(
                                cx.listener(|this, _, window, cx| this.close_search(window, cx)),
                            ),
                        )
                } else {
                    d.child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(icon("git-branch-hi", 14.).text_color(content(0.45)))
                            .child(div().min_w_0().truncate().child(label)),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap(px(2.))
                            .child(
                                header_action(
                                    "history-search",
                                    "Search history",
                                    "search",
                                    14.,
                                    true,
                                    false,
                                )
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.open_search(window, cx)),
                                ),
                            )
                            .child(
                                header_action(
                                    "history-refresh",
                                    "Refresh history",
                                    "refresh-cw",
                                    14.,
                                    can_refresh,
                                    self.loading,
                                )
                                .when(can_refresh, |d| {
                                    d.on_click(cx.listener(|this, _, _, cx| this.load(cx)))
                                }),
                            ),
                    )
                }
            }))
            .when_some(self.error.clone(), |d, error| {
                d.child(
                    div()
                        .id("history-error")
                        .role(Role::Alert)
                        .border_b_1()
                        .border_color(stroke())
                        .px_3()
                        .py_2()
                        .text_size(px(11.))
                        .text_color(rgb(RED_400))
                        .child(error),
                )
            })
    }

    fn row(&self, ix: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let item = &self.items[ix];
        let commit = &item.commit;
        let active = self.selected.as_deref() == Some(commit.sha.as_str());
        let hovered = self.hovered == Some(ix);
        let filtered = !self.query.trim().is_empty();
        let head = item.row.kind == NodeKind::Head;
        let badge = item
            .row
            .refs
            .iter()
            .find(|r| r.color.is_some())
            .or(item.row.refs.first())
            .cloned();
        let date = short_date(commit.timestamp);
        let graph = item.graph.clone();
        let width = if filtered {
            SWIMLANE_WIDTH * 2.
        } else {
            graph.width
        };
        let title: SharedString = {
            let mut t = format!("{} {}", commit.short_sha, commit.subject);
            if !commit.author.is_empty() {
                t.push_str(&format!(" — {}", commit.author));
            }
            t.into()
        };
        div()
            .id(("history-row", ix))
            .role(Role::Button)
            .aria_label(title)
            .aria_selected(active)
            .h(px(ROW))
            .w_full()
            .min_w_0()
            .flex()
            .items_stretch()
            .pl_1()
            .pr_3()
            .text_color(content(1.))
            .when(active, |d| d.bg(selection()))
            .when(!active, |d| d.hover(|s| s.bg(content(0.05))))
            .on_hover(cx.listener(move |this, hovering: &bool, _, cx| {
                let next = if *hovering {
                    Some(ix)
                } else if this.hovered == Some(ix) {
                    None
                } else {
                    this.hovered
                };
                if next != this.hovered {
                    this.hovered = next;
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, _, cx| this.select(ix, cx)))
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, _| {
                        paint_graph(&graph, bounds, filtered, head, active, hovered, window)
                    },
                )
                .w(px(width))
                .h(px(ROW))
                .flex_shrink_0(),
            )
            .child(
                div()
                    .ml_1()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .justify_center()
                    .gap(px(2.))
                    .overflow_hidden()
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .when(head, |d| d.font_weight(FontWeight::MEDIUM))
                                    .child(if commit.subject.is_empty() {
                                        commit.short_sha.clone()
                                    } else {
                                        commit.subject.clone()
                                    }),
                            )
                            .when_some(badge, |d, badge| d.child(ref_pill(&badge))),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(10.))
                            .line_height(px(14.))
                            .text_color(content(0.4))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .child(commit.author.clone()),
                            )
                            .when_some(date, |d, date| d.child(div().flex_shrink_0().child(date)))
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .font_family(crate::fonts::mono())
                                    .text_color(content(0.3))
                                    .child(commit.short_sha.clone()),
                            ),
                    ),
            )
    }
}

/// HistoryToolbar's 24px actions: 50% ink, 5% hover.
fn header_action(
    id: &'static str,
    label: &'static str,
    path: &'static str,
    side: f32,
    enabled: bool,
    spinning: bool,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .group(id)
        .size(px(24.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .when(!enabled, |d| d.opacity(0.4))
        .when(enabled, |d| d.hover(|s| s.bg(content(0.05))))
        .child(if spinning {
            spinner(id, path, side)
        } else {
            icon(path, side)
                .text_color(content(0.5))
                .when(enabled, |i| {
                    i.group_hover(id, |s| s.text_color(content(1.)))
                })
                .into_any_element()
        })
}

fn ref_pill(r: &git_graph::GraphRef) -> impl IntoElement {
    let local = r.kind == "local";
    div()
        .ml_1()
        .h(px(14.))
        .min_w_0()
        .max_w(px(104.))
        .flex_shrink_0()
        .self_center()
        .flex()
        .items_center()
        .gap(px(2.))
        .px(px(6.))
        .rounded_full()
        .text_size(px(10.))
        .line_height(px(10.))
        .map(|d| match r.color {
            Some(color) => d.bg(rgb(color)).text_color(rgb(BACKGROUND_BASE)),
            None => d.bg(content(0.1)).text_color(content(0.55)),
        })
        .when(local, |d| {
            d.child(icon("git-branch-2", 10.).text_color(match r.color {
                Some(_) => rgb(BACKGROUND_BASE).into(),
                None => content(0.55),
            }))
        })
        .child(div().min_w_0().truncate().child(r.name.clone()))
}

/// `Intl.DateTimeFormat(undefined, { month: "short", day: "numeric" })` in
/// an English locale.
fn short_date(timestamp: i64) -> Option<String> {
    use chrono::{Datelike, Local, TimeZone};
    let date = Local.timestamp_opt(timestamp, 0).single()?;
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    Some(format!("{} {}", MONTHS[date.month0() as usize], date.day()))
}

/// `color-mix(in oklab, color 70%, content)`: the detailed rows' softened ink.
fn soft(color: u32) -> Hsla {
    fn to_linear(c: f32) -> f32 {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    fn to_srgb(c: f32) -> f32 {
        if c <= 0.003_130_8 {
            c * 12.92
        } else {
            1.055 * c.powf(1. / 2.4) - 0.055
        }
    }
    fn oklab(hex: u32) -> [f32; 3] {
        let [r, g, b] = [16, 8, 0].map(|s| to_linear(((hex >> s) & 0xff) as f32 / 255.));
        let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
        let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
        let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
        [
            0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
            1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
            0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
        ]
    }
    let (a, b) = (oklab(color), oklab(TEXT));
    let [l, x, y] = [0, 1, 2].map(|i| a[i] * 0.7 + b[i] * 0.3);
    let l_ = (l + 0.396_337_78 * x + 0.215_803_76 * y).powi(3);
    let m_ = (l - 0.105_561_346 * x - 0.063_854_17 * y).powi(3);
    let s_ = (l - 0.089_484_18 * x - 1.291_485_5 * y).powi(3);
    let rgb_ = [
        4.076_741_7 * l_ - 3.307_711_6 * m_ + 0.230_969_94 * s_,
        -1.268_438 * l_ + 2.609_757_4 * m_ - 0.341_319_38 * s_,
        -0.004_196_086_3 * l_ - 0.703_418_6 * m_ + 1.707_614_7 * s_,
    ]
    .map(|c| (to_srgb(c).clamp(0., 1.) * 255.).round() as u32);
    rgb((rgb_[0] << 16) | (rgb_[1] << 8) | rgb_[2]).into()
}
/// `color-mix(in srgb, content N%, background-base)`.
fn over_base(alpha: f32) -> Hsla {
    let mix = |s: u32| {
        let c = ((TEXT >> s) & 0xff) as f32;
        let b = ((BACKGROUND_BASE >> s) & 0xff) as f32;
        (c * alpha + b * (1. - alpha)).round() as u32
    };
    rgb((mix(16) << 16) | (mix(8) << 8) | mix(0)).into()
}

/// The row SVG and index.css's `.git-history-item` cutouts: strokes in the
/// background colour, cleared or tinted on hover and selection.
fn paint_graph(
    graph: &RowGraph,
    bounds: Bounds<gpui::Pixels>,
    filtered: bool,
    head: bool,
    active: bool,
    hovered: bool,
    window: &mut Window,
) {
    let origin = bounds.origin;
    if !filtered {
        for path in &graph.paths {
            let color = soft(path.color).opacity(0.8);
            git_graph::stroke_path(&path.d, origin, path.stroke_width, color, window);
        }
    }
    let count = graph.circles.len();
    for (ix, circle) in graph.circles.iter().enumerate() {
        let cx = if filtered { SWIMLANE_WIDTH } else { circle.cx };
        let centre = origin + point(px(cx), px(circle.cy));
        let base: Hsla = rgb(BACKGROUND_BASE).into();
        let tint = if active {
            over_base(0.1)
        } else if hovered {
            over_base(0.05)
        } else {
            base
        };
        let stroke = match ix {
            0 if hovered => None,
            1 if active || hovered => Some(tint),
            _ => Some(base),
        };
        let fill_color = if head && ix + 1 == count {
            Some(tint)
        } else {
            circle.fill.map(soft)
        };
        let disc = |r: f32, color: Hsla, window: &mut Window| {
            window.paint_quad(
                fill(
                    Bounds::new(centre - point(px(r), px(r)), size(px(r * 2.), px(r * 2.))),
                    color,
                )
                .corner_radii(px(r)),
            );
        };
        if let Some(color) = fill_color {
            disc(circle.r, color, window);
        }
        if let Some(stroke) = stroke {
            let outer = circle.r + circle.stroke_width / 2.;
            window.paint_quad(gpui::quad(
                Bounds::new(
                    centre - point(px(outer), px(outer)),
                    size(px(outer * 2.), px(outer * 2.)),
                ),
                px(outer),
                gpui::transparent_black(),
                px(circle.stroke_width),
                stroke,
                gpui::BorderStyle::Solid,
            ));
        }
    }
}

impl Focusable for History {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for History {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let empty: Option<(&'static str, bool)> = if !self.available() {
            Some(("No project folder", false))
        } else if self.error.is_some() && self.items.is_empty() {
            None
        } else if self.items.is_empty() {
            Some((
                if self.loading {
                    "Loading history…"
                } else {
                    "No commits yet"
                },
                false,
            ))
        } else if self.visible.is_empty() {
            Some(("No matching commits", true))
        } else {
            None
        };
        let list_len = self.visible.len();
        let entity = cx.entity();
        div()
            .id("history")
            .role(Role::TabPanel)
            .aria_label("History")
            .track_focus(&self.focus)
            .size_full()
            .min_h_0()
            .min_w_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(self.toolbar(cx))
            .map(|d| match empty {
                Some((text, centred)) => d.child(
                    div()
                        .px_3()
                        .when(centred, |d| d.py_4().text_center())
                        .when(!centred, |d| d.py_2())
                        .text_size(px(12.))
                        .text_color(content(0.45))
                        .child(text),
                ),
                None if self.items.is_empty() => d,
                None => d.child(
                    uniform_list("history-rows", list_len, move |range, _, cx| {
                        entity.update(cx, |this, cx| {
                            range
                                .filter_map(|i| this.visible.get(i).copied())
                                .map(|ix| this.row(ix, cx).into_any_element())
                                .collect::<Vec<_>>()
                        })
                    })
                    .track_scroll(&self.scroll)
                    .flex_1()
                    .min_h_0(),
                ),
            })
    }
}

impl History {
    #[allow(dead_code)]
    fn reveal(&self, ix: usize) {
        self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn softened_lane_ink_mixes_toward_content() {
        let mixed = soft(0xFFB000);
        let pure: Hsla = rgb(0xFFB000).into();
        assert!(mixed.s < pure.s);
        assert!((over_base(0.).l - Hsla::from(rgb(BACKGROUND_BASE)).l).abs() < 0.01);
    }
}
