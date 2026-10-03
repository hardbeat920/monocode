//! Port of src/features/source-control/model/gitGraph.ts.
//!
//! Copyright (c) Microsoft Corporation.
//!
//! Portions derived from Visual Studio Code
//! (`src/vs/workbench/contrib/scm/browser/scmHistory.ts`).
//!
//! MIT License
//!
//! Permission is hereby granted, free of charge, to any person obtaining a copy
//! of this software and associated documentation files (the "Software"), to deal
//! in the Software without restriction, including without limitation the rights
//! to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
//! copies of the Software, and to permit persons to whom the Software is
//! furnished to do so, subject to the following conditions:
//!
//! The above copyright notice and this permission notice shall be included in all
//! copies or substantial portions of the Software.
//!
//! THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
//! IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
//! FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
//! AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
//! LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
//! OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
//! SOFTWARE.
//!
//! Git graph layout and per-row SVG paths. Each row is an independent 22px
//! drawing. A row's `output_swimlanes` become the next row's
//! `input_swimlanes`. Curves are SVG arcs, not cubics. The view parses the
//! path strings with [`parse_path`].

use std::collections::HashMap;

use crate::git::GitHistoryCommit;

/// A ref drawn as a pill beside a commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphRef {
    pub name: String,
    pub kind: String,
    pub color: Option<&'static str>,
}

/// A ref as git reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefName {
    pub name: String,
    pub kind: String,
}

/// `GraphCommit`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GraphCommit {
    pub sha: String,
    pub parents: Vec<String>,
    pub head: bool,
    pub refs: Vec<RefName>,
}

impl From<&GitHistoryCommit> for GraphCommit {
    fn from(commit: &GitHistoryCommit) -> Self {
        Self {
            sha: commit.sha.clone(),
            parents: commit.parents.clone(),
            head: commit.head,
            refs: commit
                .refs
                .iter()
                .map(|r| RefName {
                    name: r.name.clone(),
                    kind: r.kind.clone(),
                })
                .collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Swimlane {
    pub id: String,
    pub color: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemKind {
    Head,
    Node,
}

/// `HistoryItemViewModel`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryItemViewModel {
    pub sha: String,
    pub parent_ids: Vec<String>,
    pub kind: ItemKind,
    pub input_swimlanes: Vec<Swimlane>,
    pub output_swimlanes: Vec<Swimlane>,
    pub refs: Vec<GraphRef>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GraphPath {
    pub d: String,
    pub color: &'static str,
    pub stroke_width: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GraphCircle {
    pub cx: f32,
    pub cy: f32,
    pub r: f32,
    pub stroke_width: f32,
    pub fill: Option<&'static str>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HistoryItemGraph {
    pub width: f32,
    pub height: f32,
    pub paths: Vec<GraphPath>,
    pub circles: Vec<GraphCircle>,
    pub kind: ItemKind,
    pub circle_color: &'static str,
}

pub const SWIMLANE_HEIGHT: i64 = 22;
pub const SWIMLANE_WIDTH: i64 = 11;
const SWIMLANE_CURVE_RADIUS: i64 = 5;
const CIRCLE_RADIUS: f32 = 4.;
const CIRCLE_STROKE_WIDTH: f32 = 2.;

pub const GRAPH_ROW_PX: f32 = SWIMLANE_HEIGHT as f32;

/// Lane colors, cycled as new branches appear.
pub const COLOR_REGISTRY: [&str; 5] = ["#FFB000", "#DC267F", "#994F00", "#40B0A6", "#B66DFF"];

/// Current branch.
pub const HISTORY_ITEM_REF_COLOR: &str = "#75BEFF";
/// Upstream of the current branch.
pub const HISTORY_ITEM_REMOTE_REF_COLOR: &str = "#B180D7";
/// Fallback when a node has no swimlane color.
const HISTORY_ITEM_REF_FALLBACK: &str = HISTORY_ITEM_REF_COLOR;

pub const GRAPH_LANE_COLORS: [&str; 5] = COLOR_REGISTRY;

fn rot(index: i64, modulo: i64) -> i64 {
    ((index % modulo) + modulo) % modulo
}

fn ref_id(name: &str, kind: &str) -> String {
    format!("{kind}:{name}")
}

type ColorMap = HashMap<String, Option<&'static str>>;

/// The local ref on HEAD and its remote counterpart, as ids.
fn current_ref_ids(commits: &[GraphCommit]) -> (Option<String>, Option<String>) {
    let head = commits.iter().find(|commit| commit.head);
    let local = head.and_then(|head| head.refs.iter().find(|r| r.kind == "local"));
    let remote = local.and_then(|local| {
        head.and_then(|head| {
            head.refs.iter().find(|r| {
                r.kind == "remote"
                    && (r.name == local.name || r.name.ends_with(&format!("/{}", local.name)))
            })
        })
    });
    (
        local.map(|r| ref_id(&r.name, &r.kind)),
        remote.map(|r| ref_id(&r.name, &r.kind)),
    )
}

fn build_color_map(commits: &[GraphCommit]) -> ColorMap {
    let mut color_map = ColorMap::new();
    let (current_local, current_remote) = current_ref_ids(commits);
    if let Some(local) = current_local {
        color_map.insert(local, Some(HISTORY_ITEM_REF_COLOR));
        if let Some(remote) = current_remote {
            color_map.insert(remote, Some(HISTORY_ITEM_REMOTE_REF_COLOR));
        }
    }
    for commit in commits {
        for r in &commit.refs {
            color_map.entry(ref_id(&r.name, &r.kind)).or_insert(None);
        }
    }
    color_map
}

fn get_label_color_identifier(refs: &[RefName], color_map: &ColorMap) -> Option<&'static str> {
    refs.iter()
        .find_map(|r| color_map.get(&ref_id(&r.name, &r.kind)).copied().flatten())
}

fn compare_refs(
    a: &GraphRef,
    b: &GraphRef,
    current_local: Option<&str>,
    current_remote: Option<&str>,
) -> std::cmp::Ordering {
    let order = |r: &GraphRef| {
        let id = ref_id(&r.name, &r.kind);
        if current_local == Some(id.as_str()) {
            1
        } else if current_remote == Some(id.as_str()) {
            2
        } else if r.color.is_some() {
            4
        } else {
            99
        }
    };
    order(a).cmp(&order(b))
}

/// The lane a row's circle sits in: its own input lane, or a new one.
fn circle_index(sha: &str, input: &[Swimlane]) -> usize {
    input
        .iter()
        .position(|node| node.id == sha)
        .unwrap_or(input.len())
}

/// Assign swimlanes for `git log --topo-order` (newest first).
pub fn layout_git_graph(commits: &[GraphCommit]) -> Vec<HistoryItemViewModel> {
    let color_map = build_color_map(commits);
    let (current_local, current_remote) = current_ref_ids(commits);
    let mut color_index: i64 = -1;
    let mut view_models: Vec<HistoryItemViewModel> = Vec::with_capacity(commits.len());

    for commit in commits {
        let kind = if commit.head {
            ItemKind::Head
        } else {
            ItemKind::Node
        };
        let input_swimlanes: Vec<Swimlane> = view_models
            .last()
            .map(|last| last.output_swimlanes.clone())
            .unwrap_or_default();
        let mut output_swimlanes: Vec<Swimlane> = Vec::new();
        let parent_ids: Vec<String> = commit
            .parents
            .iter()
            .filter(|parent| !parent.is_empty())
            .cloned()
            .collect();
        let mut first_parent_added = false;

        if !parent_ids.is_empty() {
            for node in &input_swimlanes {
                if node.id == commit.sha {
                    if !first_parent_added {
                        output_swimlanes.push(Swimlane {
                            id: parent_ids[0].clone(),
                            color: get_label_color_identifier(&commit.refs, &color_map)
                                .unwrap_or(node.color),
                        });
                        first_parent_added = true;
                    }
                    continue;
                }
                output_swimlanes.push(node.clone());
            }
        }

        let start = usize::from(first_parent_added);
        for (i, parent_id) in parent_ids.iter().enumerate().skip(start) {
            let mut color = if i == 0 {
                get_label_color_identifier(&commit.refs, &color_map)
            } else {
                commits
                    .iter()
                    .find(|item| &item.sha == parent_id)
                    .and_then(|parent| get_label_color_identifier(&parent.refs, &color_map))
            };
            if color.is_none() {
                color_index = rot(color_index + 1, COLOR_REGISTRY.len() as i64);
                color = Some(COLOR_REGISTRY[color_index as usize]);
            }
            output_swimlanes.push(Swimlane {
                id: parent_id.clone(),
                color: color.unwrap_or(COLOR_REGISTRY[0]),
            });
        }

        let mut refs: Vec<GraphRef> = commit
            .refs
            .iter()
            .map(|r| {
                let id = ref_id(&r.name, &r.kind);
                let mut color = color_map.get(&id).copied().flatten();
                if color_map.contains_key(&id) && color.is_none() {
                    let circle = circle_index(&commit.sha, &input_swimlanes);
                    color = Some(if circle < output_swimlanes.len() {
                        output_swimlanes[circle].color
                    } else if circle < input_swimlanes.len() {
                        input_swimlanes[circle].color
                    } else {
                        HISTORY_ITEM_REF_FALLBACK
                    });
                }
                GraphRef {
                    name: r.name.clone(),
                    kind: r.kind.clone(),
                    color,
                }
            })
            .collect();
        refs.sort_by(|a, b| {
            compare_refs(a, b, current_local.as_deref(), current_remote.as_deref())
        });

        view_models.push(HistoryItemViewModel {
            sha: commit.sha.clone(),
            parent_ids,
            kind,
            input_swimlanes,
            output_swimlanes,
            refs,
        });
    }

    view_models
}

fn find_last_index(nodes: &[Swimlane], id: &str) -> Option<usize> {
    nodes.iter().rposition(|node| node.id == id)
}

/// `historyItemIndex`.
pub fn history_item_index(view_model: &HistoryItemViewModel) -> usize {
    circle_index(&view_model.sha, &view_model.input_swimlanes)
}

/// One row of graph paths and circles.
pub fn history_item_graph(view_model: &HistoryItemViewModel) -> HistoryItemGraph {
    const W: i64 = SWIMLANE_WIDTH;
    const H: i64 = SWIMLANE_HEIGHT;
    const R: i64 = SWIMLANE_CURVE_RADIUS;
    let input = &view_model.input_swimlanes;
    let output = &view_model.output_swimlanes;
    let parent_ids = &view_model.parent_ids;
    let input_index = input.iter().position(|node| node.id == view_model.sha);
    let circle = input_index.unwrap_or(input.len());
    let circle_color = if circle < output.len() {
        output[circle].color
    } else if circle < input.len() {
        input[circle].color
    } else {
        HISTORY_ITEM_REF_FALLBACK
    };

    let mut paths: Vec<GraphPath> = Vec::new();
    let mut push_path = |d: String, color: &'static str| {
        paths.push(GraphPath {
            d,
            color,
            stroke_width: 1.,
        });
    };

    let ci = circle as i64;
    let mut output_index = 0usize;
    for (index, lane) in input.iter().enumerate() {
        let color = lane.color;
        let i = index as i64;
        if lane.id == view_model.sha {
            if index != circle {
                push_path(
                    format!(
                        "M {} 0 A {W} {W} 0 0 1 {} {W} H {}",
                        W * (i + 1),
                        W * i,
                        W * (ci + 1)
                    ),
                    color,
                );
            } else {
                output_index += 1;
            }
        } else if output_index < output.len() && lane.id == output[output_index].id {
            if index == output_index {
                push_path(format!("M {} 0 V {H}", W * (i + 1)), color);
            } else {
                let o = output_index as i64;
                push_path(
                    format!(
                        "M {} 0 V 6 A {R} {R} 0 0 1 {} {} H {} A {R} {R} 0 0 0 {} {} V {H}",
                        W * (i + 1),
                        W * (i + 1) - R,
                        H / 2,
                        W * (o + 1) + R,
                        W * (o + 1),
                        H / 2 + R,
                    ),
                    color,
                );
            }
            output_index += 1;
        }
    }

    for parent_id in parent_ids.iter().skip(1) {
        if parent_id.is_empty() {
            continue;
        }
        let Some(parent_output) = find_last_index(output, parent_id) else {
            continue;
        };
        let color = output[parent_output].color;
        let p = parent_output as i64;
        push_path(
            format!(
                "M {} {} A {W} {W} 0 0 1 {} {H} M {} {} H {} ",
                W * p,
                H / 2,
                W * (p + 1),
                W * p,
                H / 2,
                W * (ci + 1)
            ),
            color,
        );
    }

    if let Some(input_index) = input_index {
        push_path(
            format!("M {} 0 V {}", W * (ci + 1), H / 2),
            input[input_index].color,
        );
    }

    if !parent_ids.is_empty() {
        push_path(format!("M {} {} V {H}", W * (ci + 1), H / 2), circle_color);
    }

    let cx = (W * (ci + 1)) as f32;
    let cy = W as f32;
    let mut circles = Vec::new();
    match view_model.kind {
        ItemKind::Head => {
            circles.push(GraphCircle {
                cx,
                cy,
                r: CIRCLE_RADIUS + 3.,
                stroke_width: CIRCLE_STROKE_WIDTH,
                fill: Some(circle_color),
            });
            circles.push(GraphCircle {
                cx,
                cy,
                r: CIRCLE_STROKE_WIDTH,
                stroke_width: CIRCLE_RADIUS,
                fill: None,
            });
        }
        ItemKind::Node if parent_ids.len() > 1 => {
            circles.push(GraphCircle {
                cx,
                cy,
                r: CIRCLE_RADIUS + 2.,
                stroke_width: CIRCLE_STROKE_WIDTH,
                fill: Some(circle_color),
            });
            circles.push(GraphCircle {
                cx,
                cy,
                r: CIRCLE_RADIUS - 1.,
                stroke_width: CIRCLE_STROKE_WIDTH,
                fill: Some(circle_color),
            });
        }
        ItemKind::Node => {
            circles.push(GraphCircle {
                cx,
                cy,
                r: CIRCLE_RADIUS + 1.,
                stroke_width: CIRCLE_STROKE_WIDTH,
                fill: Some(circle_color),
            });
        }
    }

    HistoryItemGraph {
        width: (W * (input.len().max(output.len()).max(1) as i64 + 1)) as f32,
        height: H as f32,
        paths,
        circles,
        kind: view_model.kind,
        circle_color,
    }
}

/// One drawing command of a graph path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathCommand {
    MoveTo(f32, f32),
    LineTo(f32, f32),
    /// `A rx ry rotation large-arc sweep x y`.
    ArcTo {
        rx: f32,
        ry: f32,
        large_arc: bool,
        sweep: bool,
        x: f32,
        y: f32,
    },
}

/// Turns the `M`, `V`, `H`, and `A` commands [`history_item_graph`] writes
/// into absolute drawing commands.
pub fn parse_path(d: &str) -> Vec<PathCommand> {
    let mut tokens = d.split_whitespace().peekable();
    let mut out = Vec::new();
    let (mut x, mut y) = (0f32, 0f32);
    let num = |tokens: &mut std::iter::Peekable<std::str::SplitWhitespace>| {
        tokens
            .next()
            .and_then(|t| t.parse::<f32>().ok())
            .unwrap_or(0.)
    };
    while let Some(command) = tokens.next() {
        match command {
            "M" => {
                x = num(&mut tokens);
                y = num(&mut tokens);
                out.push(PathCommand::MoveTo(x, y));
            }
            "V" => {
                y = num(&mut tokens);
                out.push(PathCommand::LineTo(x, y));
            }
            "H" => {
                x = num(&mut tokens);
                out.push(PathCommand::LineTo(x, y));
            }
            "A" => {
                let rx = num(&mut tokens);
                let ry = num(&mut tokens);
                let _rotation = num(&mut tokens);
                let large_arc = num(&mut tokens) != 0.;
                let sweep = num(&mut tokens) != 0.;
                x = num(&mut tokens);
                y = num(&mut tokens);
                out.push(PathCommand::ArcTo {
                    rx,
                    ry,
                    large_arc,
                    sweep,
                    x,
                    y,
                });
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: i64 = SWIMLANE_WIDTH;
    const H: i64 = SWIMLANE_HEIGHT;

    fn c(sha: &str, parents: &[&str]) -> GraphCommit {
        GraphCommit {
            sha: sha.into(),
            parents: parents.iter().map(|p| p.to_string()).collect(),
            head: false,
            refs: Vec::new(),
        }
    }

    fn with_ref(mut commit: GraphCommit, name: &str, kind: &str) -> GraphCommit {
        commit.refs.push(RefName {
            name: name.into(),
            kind: kind.into(),
        });
        commit
    }

    fn head(mut commit: GraphCommit) -> GraphCommit {
        commit.head = true;
        commit
    }

    fn ids(lanes: &[Swimlane]) -> Vec<&str> {
        lanes.iter().map(|lane| lane.id.as_str()).collect()
    }

    fn outputs(rows: &[HistoryItemViewModel]) -> Vec<(&str, Vec<&str>)> {
        rows.iter()
            .map(|row| (row.sha.as_str(), ids(&row.output_swimlanes)))
            .collect()
    }

    fn ds(graph: &HistoryItemGraph) -> Vec<&str> {
        graph.paths.iter().map(|path| path.d.as_str()).collect()
    }

    // describe("layoutGitGraph")

    #[test]
    fn keeps_a_linear_history_on_a_single_swimlane() {
        let rows = layout_git_graph(&[c("c", &["b"]), c("b", &["a"]), c("a", &[])]);
        let out: Vec<Vec<&str>> = rows.iter().map(|r| ids(&r.output_swimlanes)).collect();
        assert_eq!(out, vec![vec!["b"], vec!["a"], vec![]]);
        let input: Vec<Vec<&str>> = rows.iter().map(|r| ids(&r.input_swimlanes)).collect();
        assert_eq!(input, vec![vec![], vec!["b"], vec!["a"]]);
        assert_eq!(rows[0].output_swimlanes[0].color, COLOR_REGISTRY[0]);
    }

    #[test]
    fn opens_a_new_swimlane_for_a_merge_parent_and_curves_extra_copies_into_the_node() {
        let rows = layout_git_graph(&[
            c("M", &["A", "F"]),
            c("A", &["B"]),
            c("F", &["B"]),
            c("B", &[]),
        ]);
        assert_eq!(
            outputs(&rows),
            vec![
                ("M", vec!["A", "F"]),
                ("A", vec!["B", "F"]),
                ("F", vec!["B", "B"]),
                ("B", vec![]),
            ]
        );
        let colors: Vec<&str> = rows[0].output_swimlanes.iter().map(|l| l.color).collect();
        assert_eq!(colors, vec![COLOR_REGISTRY[0], COLOR_REGISTRY[1]]);
    }

    #[test]
    fn forks_a_second_child_onto_a_new_swimlane_while_the_first_lane_continues() {
        let rows = layout_git_graph(&[c("C", &["B"]), c("D", &["B"]), c("B", &[])]);
        assert_eq!(
            outputs(&rows),
            vec![("C", vec!["B"]), ("D", vec!["B", "B"]), ("B", vec![])]
        );
    }

    #[test]
    fn keeps_a_through_lane_vertical_across_a_stash_index_commit() {
        let rows = layout_git_graph(&[
            head(with_ref(c("A", &["M"]), "main", "local")),
            with_ref(c("S", &["W", "I"]), "stash", "local"),
            c("I", &["W"]),
            with_ref(c("W", &["F"]), "feature", "local"),
            c("F", &["M"]),
            c("M", &[]),
        ]);
        assert_eq!(
            outputs(&rows),
            vec![
                ("A", vec!["M"]),
                ("S", vec!["M", "W", "I"]),
                ("I", vec!["M", "W", "W"]),
                ("W", vec!["M", "F"]),
                ("F", vec!["M", "M"]),
                ("M", vec![]),
            ]
        );
        let index_row = history_item_graph(&rows[2]);
        assert!(ds(&index_row).contains(&format!("M {} 0 V {H}", W * 2).as_str()));
        let join = history_item_graph(&rows[5]);
        assert!(
            ds(&join).contains(&format!("M {} 0 A {W} {W} 0 0 1 {W} {W} H {W}", W * 2).as_str())
        );
    }

    #[test]
    fn colors_the_current_branch_with_the_history_item_ref_color() {
        let rows =
            layout_git_graph(&[head(with_ref(c("c", &["b"]), "main", "local")), c("b", &[])]);
        assert_eq!(rows[0].kind, ItemKind::Head);
        assert_eq!(rows[0].output_swimlanes[0].color, HISTORY_ITEM_REF_COLOR);
        assert_eq!(rows[0].refs[0].color, Some(HISTORY_ITEM_REF_COLOR));
    }

    // describe("historyItemGraph")

    #[test]
    fn draws_a_vertical_trunk_through_a_linear_row() {
        let rows = layout_git_graph(&[c("c", &["b"]), c("b", &["a"])]);
        let graph = history_item_graph(&rows[0]);
        assert_eq!(graph.height, H as f32);
        assert_eq!(graph.width, (W * 2) as f32);
        assert_eq!(ds(&graph), vec![format!("M {W} {} V {H}", H / 2)]);
        assert_eq!(
            graph.circles,
            vec![GraphCircle {
                cx: W as f32,
                cy: W as f32,
                r: 5.,
                stroke_width: 2.,
                fill: Some(COLOR_REGISTRY[0]),
            }]
        );
    }

    #[test]
    fn draws_merge_out_and_merge_in_arcs() {
        let rows = layout_git_graph(&[
            c("M", &["A", "F"]),
            c("A", &["B"]),
            c("F", &["B"]),
            c("B", &[]),
        ]);
        let merge = history_item_graph(&rows[0]);
        assert_eq!(merge.circles[0].r, 6.);
        assert_eq!(
            ds(&merge),
            vec![
                format!(
                    "M {W} {} A {W} {W} 0 0 1 {} {H} M {W} {} H {W} ",
                    H / 2,
                    W * 2,
                    H / 2
                ),
                format!("M {W} {} V {H}", H / 2),
            ]
        );
        let join = history_item_graph(&rows[3]);
        assert_eq!(
            ds(&join),
            vec![
                format!("M {} 0 A {W} {W} 0 0 1 {W} {W} H {W}", W * 2),
                format!("M {W} 0 V {}", H / 2),
            ]
        );
    }

    #[test]
    fn draws_a_pass_through_lane_beside_a_new_branch_tip() {
        let rows = layout_git_graph(&[c("C", &["B"]), c("D", &["B"]), c("B", &[])]);
        let branch_tip = history_item_graph(&rows[1]);
        assert_eq!(
            ds(&branch_tip),
            vec![
                format!("M {W} 0 V {H}"),
                format!("M {} {} V {H}", W * 2, H / 2),
            ]
        );
    }

    #[test]
    fn draws_elbow_arcs_when_a_through_lane_shifts_left() {
        let rows = layout_git_graph(&[
            c("M", &["A", "F"]),
            c("C", &["X"]),
            c("A", &["B"]),
            c("F", &["B"]),
            c("B", &["R"]),
            c("X", &["R"]),
            c("R", &[]),
        ]);
        assert_eq!(ids(&rows[4].input_swimlanes), vec!["B", "B", "X"]);
        assert_eq!(ids(&rows[4].output_swimlanes), vec!["R", "X"]);
        let shifted = history_item_graph(&rows[4]);
        let expected = [
            format!("M {} 0", W * 3),
            "V 6".to_string(),
            format!("A 5 5 0 0 1 {} {}", W * 3 - 5, H / 2),
            format!("H {}", W * 2 + 5),
            format!("A 5 5 0 0 0 {} {}", W * 2, H / 2 + 5),
            format!("V {H}"),
        ]
        .join(" ");
        assert!(ds(&shifted).contains(&expected.as_str()));
    }

    #[test]
    fn draws_head_as_an_outer_disc_plus_cutout_inner_circle() {
        let rows = layout_git_graph(&[head(c("c", &["b"]))]);
        let graph = history_item_graph(&rows[0]);
        assert_eq!(graph.kind, ItemKind::Head);
        let circles: Vec<(f32, f32, Option<&str>)> = graph
            .circles
            .iter()
            .map(|circle| (circle.r, circle.stroke_width, circle.fill))
            .collect();
        assert_eq!(
            circles,
            vec![(7., 2., Some(COLOR_REGISTRY[0])), (2., 4., None)]
        );
    }

    #[test]
    fn parses_the_path_commands_it_writes() {
        let commands = parse_path("M 22 0 A 11 11 0 0 1 11 11 H 11");
        assert_eq!(
            commands,
            vec![
                PathCommand::MoveTo(22., 0.),
                PathCommand::ArcTo {
                    rx: 11.,
                    ry: 11.,
                    large_arc: false,
                    sweep: true,
                    x: 11.,
                    y: 11.,
                },
                PathCommand::LineTo(11., 11.),
            ]
        );
        assert_eq!(
            parse_path("M 11 11 V 22"),
            vec![PathCommand::MoveTo(11., 11.), PathCommand::LineTo(11., 22.)]
        );
    }
}
