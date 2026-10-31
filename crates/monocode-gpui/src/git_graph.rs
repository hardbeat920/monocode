//! Port of src/features/source-control/model/gitGraph.ts, itself derived from
//! Visual Studio Code's `scmHistory.ts` (MIT, Copyright (c) Microsoft
//! Corporation). Previous row `output` lanes become the next row's `input`;
//! curves are SVG arcs, not cubics.
use gpui::{Hsla, PathBuilder, Pixels, Point, Window, point, px};
use std::collections::HashMap;

pub(crate) const SWIMLANE_HEIGHT: f32 = 22.;
pub(crate) const SWIMLANE_WIDTH: f32 = 11.;
const SWIMLANE_CURVE_RADIUS: f32 = 5.;
const CIRCLE_RADIUS: f32 = 4.;
const CIRCLE_STROKE_WIDTH: f32 = 2.;

/// Lane colors, cycled as new branches appear.
pub(crate) const COLOR_REGISTRY: [u32; 5] = [0xFFB000, 0xDC267F, 0x994F00, 0x40B0A6, 0xB66DFF];
/// Current branch.
pub(crate) const HISTORY_ITEM_REF_COLOR: u32 = 0x75BEFF;
/// Upstream of the current branch.
pub(crate) const HISTORY_ITEM_REMOTE_REF_COLOR: u32 = 0xB180D7;
const FALLBACK: u32 = HISTORY_ITEM_REF_COLOR;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GraphCommit {
    pub sha: String,
    pub parents: Vec<String>,
    pub head: bool,
    /// `(name, kind)`.
    pub refs: Vec<(String, String)>,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GraphRef {
    pub name: String,
    pub kind: String,
    pub color: Option<u32>,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Swimlane {
    pub id: String,
    pub color: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NodeKind {
    Head,
    Node,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Row {
    pub sha: String,
    pub parent_ids: Vec<String>,
    pub kind: NodeKind,
    pub input: Vec<Swimlane>,
    pub output: Vec<Swimlane>,
    pub refs: Vec<GraphRef>,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GraphPath {
    pub d: String,
    pub color: u32,
    pub stroke_width: f32,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Circle {
    pub cx: f32,
    pub cy: f32,
    pub r: f32,
    pub stroke_width: f32,
    pub fill: Option<u32>,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RowGraph {
    pub width: f32,
    pub height: f32,
    pub paths: Vec<GraphPath>,
    pub circles: Vec<Circle>,
    pub kind: NodeKind,
}

fn ref_id(name: &str, kind: &str) -> String {
    format!("{kind}:{name}")
}
fn current_ref_ids(commits: &[GraphCommit]) -> (Option<String>, Option<String>) {
    let head = commits.iter().find(|c| c.head);
    let local = head.and_then(|h| h.refs.iter().find(|(_, kind)| kind == "local"));
    let remote = local.and_then(|(local, _)| {
        head?.refs.iter().find(|(name, kind)| {
            kind == "remote" && (name == local || name.ends_with(&format!("/{local}")))
        })
    });
    (
        local.map(|(n, k)| ref_id(n, k)),
        remote.map(|(n, k)| ref_id(n, k)),
    )
}
/// Ordered as the TS `Map`: insertion order, colorless entries kept.
fn build_color_map(commits: &[GraphCommit]) -> HashMap<String, Option<u32>> {
    let mut map = HashMap::new();
    let (local, remote) = current_ref_ids(commits);
    if let Some(local) = local {
        map.insert(local, Some(HISTORY_ITEM_REF_COLOR));
        if let Some(remote) = remote {
            map.insert(remote, Some(HISTORY_ITEM_REMOTE_REF_COLOR));
        }
    }
    for commit in commits {
        for (name, kind) in &commit.refs {
            map.entry(ref_id(name, kind)).or_insert(None);
        }
    }
    map
}
fn label_color(refs: &[(String, String)], map: &HashMap<String, Option<u32>>) -> Option<u32> {
    refs.iter()
        .find_map(|(name, kind)| map.get(&ref_id(name, kind)).copied().flatten())
}

/// Assign swimlanes for `git log --topo-order` (newest first).
pub(crate) fn layout(commits: &[GraphCommit]) -> Vec<Row> {
    let colors = build_color_map(commits);
    let (current_local, current_remote) = current_ref_ids(commits);
    let by_sha: HashMap<&str, &GraphCommit> = commits.iter().map(|c| (c.sha.as_str(), c)).collect();
    let mut color_index: isize = -1;
    let mut rows: Vec<Row> = Vec::with_capacity(commits.len());
    for commit in commits {
        let kind = if commit.head {
            NodeKind::Head
        } else {
            NodeKind::Node
        };
        let input = rows.last().map(|r| r.output.clone()).unwrap_or_default();
        let mut output = Vec::new();
        let parent_ids: Vec<String> = commit
            .parents
            .iter()
            .filter(|p| !p.is_empty())
            .cloned()
            .collect();
        let mut first_parent_added = false;
        if !parent_ids.is_empty() {
            for node in &input {
                if node.id == commit.sha {
                    if !first_parent_added {
                        output.push(Swimlane {
                            id: parent_ids[0].clone(),
                            color: label_color(&commit.refs, &colors).unwrap_or(node.color),
                        });
                        first_parent_added = true;
                    }
                    continue;
                }
                output.push(node.clone());
            }
        }
        for (i, parent) in parent_ids
            .iter()
            .enumerate()
            .skip(usize::from(first_parent_added))
        {
            let color = if i == 0 {
                label_color(&commit.refs, &colors)
            } else {
                by_sha
                    .get(parent.as_str())
                    .and_then(|p| label_color(&p.refs, &colors))
            };
            let color = color.unwrap_or_else(|| {
                color_index = (color_index + 1).rem_euclid(COLOR_REGISTRY.len() as isize);
                COLOR_REGISTRY[color_index as usize]
            });
            output.push(Swimlane {
                id: parent.clone(),
                color,
            });
        }
        let circle_index = input
            .iter()
            .position(|n| n.id == commit.sha)
            .unwrap_or(input.len());
        let mut refs: Vec<GraphRef> = commit
            .refs
            .iter()
            .map(|(name, kind)| {
                let id = ref_id(name, kind);
                let color = match colors.get(&id) {
                    Some(Some(color)) => Some(*color),
                    Some(None) => Some(
                        output
                            .get(circle_index)
                            .or_else(|| input.get(circle_index))
                            .map_or(FALLBACK, |lane| lane.color),
                    ),
                    None => None,
                };
                GraphRef {
                    name: name.clone(),
                    kind: kind.clone(),
                    color,
                }
            })
            .collect();
        let order = |r: &GraphRef| {
            let id = ref_id(&r.name, &r.kind);
            if current_local.as_ref() == Some(&id) {
                1
            } else if current_remote.as_ref() == Some(&id) {
                2
            } else if r.color.is_some() {
                4
            } else {
                99
            }
        };
        refs.sort_by_key(order);
        rows.push(Row {
            sha: commit.sha.clone(),
            parent_ids,
            kind,
            input,
            output,
            refs,
        });
    }
    rows
}

/// One row of graph paths and circles. Taller rows extend lanes above and
/// below the node while keeping the circles and curve radii at their size.
pub(crate) fn row_graph(row: &Row, row_height: f32) -> RowGraph {
    const W: f32 = SWIMLANE_WIDTH;
    const R: f32 = SWIMLANE_CURVE_RADIUS;
    let center = row_height / 2.;
    let pad = (row_height - SWIMLANE_HEIGHT) / 2.;
    let (input, output) = (&row.input, &row.output);
    let input_index = input.iter().position(|n| n.id == row.sha);
    let circle_index = input_index.unwrap_or(input.len());
    let circle_color = output
        .get(circle_index)
        .or_else(|| input.get(circle_index))
        .map_or(FALLBACK, |lane| lane.color);
    let mut paths = Vec::new();
    let mut push = |d: String, color: u32| {
        paths.push(GraphPath {
            d,
            color,
            stroke_width: 1.,
        })
    };
    let n = |v: f32| fmt(v);
    let mut out_ix = 0;
    for (index, lane) in input.iter().enumerate() {
        let i = index as f32;
        if lane.id == row.sha {
            if index != circle_index {
                let mut d = vec![format!("M {} 0", n(W * (i + 1.)))];
                if pad > 0. {
                    d.push(format!("V {}", n(pad)));
                }
                d.push(format!("A {W} {W} 0 0 1 {} {}", n(W * i), n(center)));
                d.push(format!("H {}", n(W * (circle_index as f32 + 1.))));
                push(d.join(" "), lane.color);
            } else {
                out_ix += 1;
            }
        } else if out_ix < output.len() && lane.id == output[out_ix].id {
            let o = out_ix as f32;
            if index == out_ix {
                push(
                    format!("M {} 0 V {}", n(W * (i + 1.)), n(row_height)),
                    lane.color,
                );
            } else {
                push(
                    [
                        format!("M {} 0", n(W * (i + 1.))),
                        format!("V {}", n(center - R)),
                        format!("A {R} {R} 0 0 1 {} {}", n(W * (i + 1.) - R), n(center)),
                        format!("H {}", n(W * (o + 1.) + R)),
                        format!("A {R} {R} 0 0 0 {} {}", n(W * (o + 1.)), n(center + R)),
                        format!("V {}", n(row_height)),
                    ]
                    .join(" "),
                    lane.color,
                );
            }
            out_ix += 1;
        }
    }
    for parent in row.parent_ids.iter().skip(1) {
        let Some(ix) = output.iter().rposition(|lane| &lane.id == parent) else {
            continue;
        };
        let p = ix as f32;
        let mut d = vec![
            format!("M {} {}", n(W * p), n(center)),
            format!("A {W} {W} 0 0 1 {} {}", n(W * (p + 1.)), n(center + W)),
        ];
        if pad > 0. {
            d.push(format!("V {}", n(row_height)));
        }
        d.push(format!("M {} {}", n(W * p), n(center)));
        d.push(format!("H {} ", n(W * (circle_index as f32 + 1.))));
        push(d.join(" "), output[ix].color);
    }
    let cx = W * (circle_index as f32 + 1.);
    if let Some(ix) = input_index {
        push(format!("M {} 0 V {}", n(cx), n(center)), input[ix].color);
    }
    if !row.parent_ids.is_empty() {
        push(
            format!("M {} {} V {}", n(cx), n(center), n(row_height)),
            circle_color,
        );
    }
    let circle = |r: f32, stroke_width: f32, fill: Option<u32>| Circle {
        cx,
        cy: center,
        r,
        stroke_width,
        fill,
    };
    let circles = if row.kind == NodeKind::Head {
        vec![
            circle(CIRCLE_RADIUS + 3., CIRCLE_STROKE_WIDTH, Some(circle_color)),
            circle(CIRCLE_STROKE_WIDTH, CIRCLE_RADIUS, None),
        ]
    } else if row.parent_ids.len() > 1 {
        vec![
            circle(CIRCLE_RADIUS + 2., CIRCLE_STROKE_WIDTH, Some(circle_color)),
            circle(CIRCLE_RADIUS - 1., CIRCLE_STROKE_WIDTH, Some(circle_color)),
        ]
    } else {
        vec![circle(
            CIRCLE_RADIUS + 1.,
            CIRCLE_STROKE_WIDTH,
            Some(circle_color),
        )]
    };
    RowGraph {
        width: W * (input.len().max(output.len()).max(1) as f32 + 1.),
        height: row_height,
        paths,
        circles,
        kind: row.kind,
    }
}
/// JavaScript's number formatting for the coordinates used here.
fn fmt(v: f32) -> String {
    if v.fract() == 0. {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

/// Strokes the `M`/`V`/`H`/`A` subset `row_graph` emits, relative to `origin`.
pub(crate) fn stroke_path(
    d: &str,
    origin: Point<Pixels>,
    width: f32,
    color: Hsla,
    window: &mut Window,
) {
    let mut builder = PathBuilder::stroke(px(width));
    let mut tokens = d.split_whitespace();
    let mut at = point(0f32, 0f32);
    let to = |p: Point<f32>| origin + point(px(p.x), px(p.y));
    let num = |t: &mut std::str::SplitWhitespace| -> f32 {
        t.next().and_then(|v| v.parse().ok()).unwrap_or(0.)
    };
    while let Some(command) = tokens.next() {
        match command {
            "M" => {
                at = point(num(&mut tokens), num(&mut tokens));
                builder.move_to(to(at));
            }
            "V" => {
                at.y = num(&mut tokens);
                builder.line_to(to(at));
            }
            "H" => {
                at.x = num(&mut tokens);
                builder.line_to(to(at));
            }
            "A" => {
                let (rx, ry) = (num(&mut tokens), num(&mut tokens));
                let rotation = num(&mut tokens);
                let large = num(&mut tokens) != 0.;
                let sweep = num(&mut tokens) != 0.;
                at = point(num(&mut tokens), num(&mut tokens));
                builder.arc_to(point(px(rx), px(ry)), px(rotation), large, sweep, to(at));
            }
            _ => {}
        }
    }
    if let Ok(path) = builder.build() {
        window.paint_path(path, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn c(sha: &str, parents: &[&str]) -> GraphCommit {
        GraphCommit {
            sha: sha.into(),
            parents: parents.iter().map(|p| p.to_string()).collect(),
            head: false,
            refs: Vec::new(),
        }
    }
    fn with_ref(mut commit: GraphCommit, name: &str, head: bool) -> GraphCommit {
        commit.refs.push((name.into(), "local".into()));
        commit.head = head;
        commit
    }
    fn ids(rows: &[Row]) -> Vec<(String, Vec<String>)> {
        rows.iter()
            .map(|r| {
                (
                    r.sha.clone(),
                    r.output.iter().map(|l| l.id.clone()).collect(),
                )
            })
            .collect()
    }
    fn expect(rows: &[Row], want: &[(&str, &[&str])]) {
        let want: Vec<(String, Vec<String>)> = want
            .iter()
            .map(|(s, o)| (s.to_string(), o.iter().map(|x| x.to_string()).collect()))
            .collect();
        assert_eq!(ids(rows), want);
    }
    fn ds(graph: &RowGraph) -> Vec<&str> {
        graph.paths.iter().map(|p| p.d.as_str()).collect()
    }
    fn merge() -> Vec<Row> {
        layout(&[
            c("M", &["A", "F"]),
            c("A", &["B"]),
            c("F", &["B"]),
            c("B", &[]),
        ])
    }

    #[test]
    fn linear_history_keeps_one_swimlane() {
        let rows = layout(&[c("c", &["b"]), c("b", &["a"]), c("a", &[])]);
        expect(&rows, &[("c", &["b"]), ("b", &["a"]), ("a", &[])]);
        assert_eq!(rows[1].input[0].id, "b");
        assert_eq!(rows[0].output[0].color, COLOR_REGISTRY[0]);
    }
    #[test]
    fn merge_parent_opens_a_lane() {
        let rows = merge();
        expect(
            &rows,
            &[
                ("M", &["A", "F"]),
                ("A", &["B", "F"]),
                ("F", &["B", "B"]),
                ("B", &[]),
            ],
        );
        assert_eq!(
            rows[0].output.iter().map(|l| l.color).collect::<Vec<_>>(),
            COLOR_REGISTRY[..2]
        );
    }
    #[test]
    fn second_child_forks_and_stash_index_keeps_through_lane() {
        let rows = layout(&[c("C", &["B"]), c("D", &["B"]), c("B", &[])]);
        expect(&rows, &[("C", &["B"]), ("D", &["B", "B"]), ("B", &[])]);
        let tip = row_graph(&rows[1], SWIMLANE_HEIGHT);
        assert_eq!(ds(&tip), ["M 11 0 V 22", "M 22 11 V 22"]);

        let rows = layout(&[
            with_ref(c("A", &["M"]), "main", true),
            with_ref(c("S", &["W", "I"]), "stash", false),
            c("I", &["W"]),
            with_ref(c("W", &["F"]), "feature", false),
            c("F", &["M"]),
            c("M", &[]),
        ]);
        expect(
            &rows,
            &[
                ("A", &["M"]),
                ("S", &["M", "W", "I"]),
                ("I", &["M", "W", "W"]),
                ("W", &["M", "F"]),
                ("F", &["M", "M"]),
                ("M", &[]),
            ],
        );
        assert!(ds(&row_graph(&rows[2], SWIMLANE_HEIGHT)).contains(&"M 22 0 V 22"));
        assert!(
            ds(&row_graph(&rows[5], SWIMLANE_HEIGHT)).contains(&"M 22 0 A 11 11 0 0 1 11 11 H 11")
        );
    }
    #[test]
    fn current_branch_uses_the_ref_color() {
        let rows = layout(&[with_ref(c("c", &["b"]), "main", true), c("b", &[])]);
        assert_eq!(rows[0].kind, NodeKind::Head);
        assert_eq!(rows[0].output[0].color, HISTORY_ITEM_REF_COLOR);
        assert_eq!(rows[0].refs[0].color, Some(HISTORY_ITEM_REF_COLOR));
    }
    #[test]
    fn taller_rows_extend_lanes_not_nodes() {
        let rows = merge();
        let m = row_graph(&rows[0], 44.);
        assert_eq!(m.height, 44.);
        assert_eq!(
            m.circles.iter().map(|c| (c.cy, c.r)).collect::<Vec<_>>(),
            [(22., 6.), (22., 3.)]
        );
        assert!(m.paths[0].d.contains("A 11 11 0 0 1 22 33 V 44"));
        let join = row_graph(&rows[3], 44.);
        assert!(join.paths[0].d.contains("M 22 0 V 11 A 11 11 0 0 1 11 22"));
    }
    #[test]
    fn linear_merge_and_join_paths() {
        let rows = layout(&[c("c", &["b"]), c("b", &["a"])]);
        let g = row_graph(&rows[0], SWIMLANE_HEIGHT);
        assert_eq!(g.width, 22.);
        assert_eq!(ds(&g), ["M 11 11 V 22"]);
        assert_eq!(
            g.circles,
            [Circle {
                cx: 11.,
                cy: 11.,
                r: 5.,
                stroke_width: 2.,
                fill: Some(COLOR_REGISTRY[0])
            }]
        );
        let rows = merge();
        assert_eq!(
            ds(&row_graph(&rows[0], SWIMLANE_HEIGHT)),
            ["M 11 11 A 11 11 0 0 1 22 22 M 11 11 H 11 ", "M 11 11 V 22"]
        );
        assert_eq!(
            ds(&row_graph(&rows[3], SWIMLANE_HEIGHT)),
            ["M 22 0 A 11 11 0 0 1 11 11 H 11", "M 11 0 V 11"]
        );
    }
    #[test]
    fn shifted_lane_draws_elbows_and_head_has_cutout() {
        let rows = layout(&[
            c("M", &["A", "F"]),
            c("C", &["X"]),
            c("A", &["B"]),
            c("F", &["B"]),
            c("B", &["R"]),
            c("X", &["R"]),
            c("R", &[]),
        ]);
        let ids: Vec<_> = rows[4].input.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(ids, ["B", "B", "X"]);
        assert!(
            ds(&row_graph(&rows[4], SWIMLANE_HEIGHT))
                .contains(&"M 33 0 V 6 A 5 5 0 0 1 28 11 H 27 A 5 5 0 0 0 22 16 V 22")
        );
        let mut head = c("c", &["b"]);
        head.head = true;
        let g = row_graph(&layout(&[head])[0], SWIMLANE_HEIGHT);
        assert_eq!(
            g.circles
                .iter()
                .map(|c| (c.r, c.stroke_width, c.fill))
                .collect::<Vec<_>>(),
            [(7., 2., Some(COLOR_REGISTRY[0])), (2., 4., None)]
        );
    }
}
