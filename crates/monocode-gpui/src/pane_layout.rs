//! Port of src/features/workspace/model/layout.ts: a tab's split tree.
//! Same-direction splits share a group so new panes divide space equally
//! until a sash moves. A pane dropped on another pane's edge reorders
//! same-axis siblings, nests a perpendicular split, or relocates into the
//! target's group. Splits are addressed by their child-index path.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum SplitDir {
    Right,
    Down,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Place {
    Before,
    After,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub(crate) enum PaneNode {
    Leaf {
        id: i64,
    },
    Split {
        dir: SplitDir,
        children: Vec<PaneNode>,
        sizes: Vec<f32>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub(crate) struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}
impl Rect {
    pub const FULL: Rect = Rect {
        x: 0.,
        y: 0.,
        w: 1.,
        h: 1.,
    };
    pub fn lerp(self, to: Rect, t: f32) -> Rect {
        Rect {
            x: self.x + (to.x - self.x) * t,
            y: self.y + (to.y - self.y) * t,
            w: self.w + (to.w - self.w) * t,
            h: self.h + (to.h - self.h) * t,
        }
    }
    fn child(self, dir: SplitDir, offset: f32, size: f32) -> Rect {
        match dir {
            SplitDir::Right => Rect {
                x: self.x + offset * self.w,
                w: size * self.w,
                ..self
            },
            SplitDir::Down => Rect {
                y: self.y + offset * self.h,
                h: size * self.h,
                ..self
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LayoutLeaf {
    pub id: i64,
    pub rect: Rect,
    /// The axis its parent split runs along.
    pub axis: SplitDir,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LayoutSash {
    pub path: Vec<usize>,
    pub index: usize,
    pub dir: SplitDir,
    pub group: Rect,
    pub boundary: f32,
}

const MIN_SIZE: f32 = 0.08;

pub(crate) fn leaf(id: i64) -> PaneNode {
    PaneNode::Leaf { id }
}
fn split(dir: SplitDir, children: Vec<PaneNode>) -> PaneNode {
    let sizes = equal_sizes(children.len());
    PaneNode::Split {
        dir,
        children,
        sizes,
    }
}
fn equal_sizes(n: usize) -> Vec<f32> {
    vec![1. / n as f32; n]
}
fn normalize(sizes: &[f32]) -> Vec<f32> {
    let total: f32 = sizes.iter().sum();
    if total <= 0. {
        return equal_sizes(sizes.len());
    }
    sizes.iter().map(|s| s / total).collect()
}
fn edge_split(edge: Edge) -> (SplitDir, Place) {
    match edge {
        Edge::Left => (SplitDir::Right, Place::Before),
        Edge::Right => (SplitDir::Right, Place::After),
        Edge::Top => (SplitDir::Down, Place::Before),
        Edge::Bottom => (SplitDir::Down, Place::After),
    }
}

/// The edge nearest the point, from the pane's centre.
pub(crate) fn edge_from_point(x: f32, y: f32, rect: Rect) -> Edge {
    let nx = if rect.w <= 0. {
        0.
    } else {
        (x - rect.x) / rect.w - 0.5
    };
    let ny = if rect.h <= 0. {
        0.
    } else {
        (y - rect.y) / rect.h - 0.5
    };
    if nx.abs() > ny.abs() {
        if nx < 0. { Edge::Left } else { Edge::Right }
    } else if ny < 0. {
        Edge::Top
    } else {
        Edge::Bottom
    }
}

impl PaneNode {
    pub fn leaf_ids(&self) -> Vec<i64> {
        match self {
            PaneNode::Leaf { id } => vec![*id],
            PaneNode::Split { children, .. } => children.iter().flat_map(Self::leaf_ids).collect(),
        }
    }
    pub fn contains(&self, id: i64) -> bool {
        match self {
            PaneNode::Leaf { id: leaf } => *leaf == id,
            PaneNode::Split { children, .. } => children.iter().any(|c| c.contains(id)),
        }
    }
    pub fn first_leaf(&self) -> i64 {
        match self {
            PaneNode::Leaf { id } => *id,
            PaneNode::Split { children, .. } => children[0].first_leaf(),
        }
    }
    pub fn is_split(&self) -> bool {
        matches!(self, PaneNode::Split { .. })
    }

    /// cmd-d / shift-cmd-d: a new pane after the focused one.
    pub fn split_pane(&self, focused: i64, dir: SplitDir, new: i64) -> PaneNode {
        match self {
            PaneNode::Leaf { id } if *id == focused => split(dir, vec![self.clone(), leaf(new)]),
            PaneNode::Leaf { .. } => self.clone(),
            PaneNode::Split {
                dir: own,
                children,
                sizes,
            } => {
                if let Some(ix) = children
                    .iter()
                    .position(|c| matches!(c, PaneNode::Leaf { id } if *id == focused))
                {
                    let mut children = children.clone();
                    if *own == dir {
                        children.insert(ix + 1, leaf(new));
                        return split(dir, children);
                    }
                    children[ix] = split(dir, vec![children[ix].clone(), leaf(new)]);
                    return PaneNode::Split {
                        dir: *own,
                        children,
                        sizes: sizes.clone(),
                    };
                }
                PaneNode::Split {
                    dir: *own,
                    children: children
                        .iter()
                        .map(|c| c.split_pane(focused, dir, new))
                        .collect(),
                    sizes: sizes.clone(),
                }
            }
        }
    }

    /// Swap one leaf for another, keeping the tree intact.
    pub fn replace_leaf(&self, from: i64, to: i64) -> PaneNode {
        match self {
            PaneNode::Leaf { id } if *id == from => leaf(to),
            PaneNode::Leaf { .. } => self.clone(),
            PaneNode::Split {
                dir,
                children,
                sizes,
            } => PaneNode::Split {
                dir: *dir,
                children: children.iter().map(|c| c.replace_leaf(from, to)).collect(),
                sizes: sizes.clone(),
            },
        }
    }

    /// Drop a leaf; a parent left with one child collapses to it.
    pub fn remove(&self, target: i64) -> Option<PaneNode> {
        match self {
            PaneNode::Leaf { id } => (*id != target).then(|| self.clone()),
            PaneNode::Split {
                dir,
                children,
                sizes,
            } => {
                let kept: Vec<_> = children
                    .iter()
                    .zip(sizes)
                    .filter_map(|(c, s)| c.remove(target).map(|c| (c, *s)))
                    .collect();
                match kept.len() {
                    0 => None,
                    1 => kept.into_iter().next().map(|(c, _)| c),
                    _ => {
                        let sizes: Vec<_> = kept.iter().map(|(_, s)| *s).collect();
                        Some(PaneNode::Split {
                            dir: *dir,
                            children: kept.into_iter().map(|(c, _)| c).collect(),
                            sizes: normalize(&sizes),
                        })
                    }
                }
            }
        }
    }

    /// The pane to focus after closing `id`: a neighbour's first leaf.
    pub fn sibling(&self, id: i64) -> Option<i64> {
        let PaneNode::Split { children, .. } = self else {
            return None;
        };
        if let Some(ix) = children
            .iter()
            .position(|c| matches!(c, PaneNode::Leaf { id: l } if *l == id))
        {
            let neighbour = if ix > 0 {
                children.get(ix - 1)
            } else {
                children.get(ix + 1)
            };
            return neighbour.map(Self::first_leaf);
        }
        children.iter().find_map(|c| c.sibling(id))
    }

    /// Move the sash between `index` and `index + 1` of the split at `path`
    /// to `boundary`, a 0–1 position within its group. It may pass the
    /// `sash_range` a little; callers clamp drags to it.
    pub fn place_sash(&mut self, path: &[usize], index: usize, boundary: f32) {
        if let Some(sizes) = self.sizes_mut(path) {
            *sizes = sizes_at_boundary(sizes, index, boundary, MIN_SIZE / 4.);
        }
    }

    /// The sash's current boundary and the range it rests in.
    pub fn sash_range(&self, path: &[usize], index: usize) -> Option<(f32, f32, f32)> {
        let PaneNode::Split {
            children, sizes, ..
        } = self
        else {
            return None;
        };
        if let Some((first, rest)) = path.split_first() {
            return children.get(*first)?.sash_range(rest, index);
        }
        if index + 1 >= sizes.len() {
            return None;
        }
        let before: f32 = sizes[..index].iter().sum();
        let pair = sizes[index] + sizes[index + 1];
        let min = MIN_SIZE.min(pair / 2.);
        Some((before + sizes[index], before + min, before + pair - min))
    }

    fn sizes_mut(&mut self, path: &[usize]) -> Option<&mut Vec<f32>> {
        let PaneNode::Split {
            children, sizes, ..
        } = self
        else {
            return None;
        };
        match path.split_first() {
            Some((first, rest)) => children.get_mut(*first)?.sizes_mut(rest),
            None => Some(sizes),
        }
    }

    pub fn leaves(&self) -> Vec<LayoutLeaf> {
        let mut out = Vec::new();
        self.collect_leaves(Rect::FULL, SplitDir::Right, &mut out);
        out
    }
    fn collect_leaves(&self, rect: Rect, axis: SplitDir, out: &mut Vec<LayoutLeaf>) {
        match self {
            PaneNode::Leaf { id } => out.push(LayoutLeaf {
                id: *id,
                rect,
                axis,
            }),
            PaneNode::Split {
                dir,
                children,
                sizes,
            } => {
                let mut offset = 0.;
                for (child, size) in children.iter().zip(sizes) {
                    child.collect_leaves(rect.child(*dir, offset, *size), *dir, out);
                    offset += size;
                }
            }
        }
    }
    pub fn sashes(&self) -> Vec<LayoutSash> {
        let mut out = Vec::new();
        self.collect_sashes(Rect::FULL, &mut Vec::new(), &mut out);
        out
    }
    fn collect_sashes(&self, rect: Rect, path: &mut Vec<usize>, out: &mut Vec<LayoutSash>) {
        let PaneNode::Split {
            dir,
            children,
            sizes,
        } = self
        else {
            return;
        };
        let mut offset = 0.;
        for (ix, (child, size)) in children.iter().zip(sizes).enumerate() {
            if ix > 0 {
                out.push(LayoutSash {
                    path: path.clone(),
                    index: ix - 1,
                    dir: *dir,
                    group: rect,
                    boundary: offset,
                });
            }
            path.push(ix);
            child.collect_sashes(rect.child(*dir, offset, *size), path, out);
            path.pop();
            offset += size;
        }
    }

    /// cmd-opt-arrows: the adjacent pane, preferring ones sharing an edge.
    pub fn neighbor(&self, focused: i64, dir: Edge) -> Option<i64> {
        let panes = self.leaves();
        let c = panes.iter().find(|p| p.id == focused)?.rect;
        let overlap = |a0: f32, a1: f32, b0: f32, b1: f32| (a1.min(b1) - a0.max(b0)).max(0.);
        let mut best: Option<(i64, u8, f32, f32)> = None;
        for pane in &panes {
            if pane.id == focused {
                continue;
            }
            let r = pane.rect;
            let (gap, perp) = match dir {
                Edge::Left if r.x + r.w <= c.x + 1e-4 => {
                    (c.x - (r.x + r.w), overlap(c.y, c.y + c.h, r.y, r.y + r.h))
                }
                Edge::Right if r.x >= c.x + c.w - 1e-4 => {
                    (r.x - (c.x + c.w), overlap(c.y, c.y + c.h, r.y, r.y + r.h))
                }
                Edge::Top if r.y + r.h <= c.y + 1e-4 => {
                    (c.y - (r.y + r.h), overlap(c.x, c.x + c.w, r.x, r.x + r.w))
                }
                Edge::Bottom if r.y >= c.y + c.h - 1e-4 => {
                    (r.y - (c.y + c.h), overlap(c.x, c.x + c.w, r.x, r.x + r.w))
                }
                _ => continue,
            };
            let hit = if perp > 0. { 0 } else { 1 };
            let better = best.is_none_or(|(_, bh, bg, bo)| {
                hit < bh || (hit == bh && gap < bg) || (hit == bh && gap == bg && perp > bo)
            });
            if better {
                best = Some((pane.id, hit, gap, perp));
            }
        }
        best.map(|b| b.0)
    }

    fn leaf_parent(&self, target: i64) -> Option<(Vec<usize>, usize, SplitDir)> {
        fn walk(
            node: &PaneNode,
            target: i64,
            path: &mut Vec<usize>,
        ) -> Option<(Vec<usize>, usize, SplitDir)> {
            let PaneNode::Split { dir, children, .. } = node else {
                return None;
            };
            for (ix, child) in children.iter().enumerate() {
                if matches!(child, PaneNode::Leaf { id } if *id == target) {
                    return Some((path.clone(), ix, *dir));
                }
                path.push(ix);
                let found = walk(child, target, path);
                path.pop();
                if found.is_some() {
                    return found;
                }
            }
            None
        }
        walk(self, target, &mut Vec::new())
    }
    fn at_path(&mut self, path: &[usize]) -> &mut PaneNode {
        match path.split_first() {
            Some((first, rest)) => match self {
                PaneNode::Split { children, .. } => children[*first].at_path(rest),
                PaneNode::Leaf { .. } => self,
            },
            None => self,
        }
    }
    fn insert_beside(&self, target: i64, incoming: PaneNode, place: Place) -> PaneNode {
        let PaneNode::Split {
            dir,
            children,
            sizes,
        } = self
        else {
            return self.clone();
        };
        if let Some(ix) = children
            .iter()
            .position(|c| matches!(c, PaneNode::Leaf { id } if *id == target))
        {
            let mut children = children.clone();
            let mut sizes = sizes.clone();
            let share = sizes[ix] / 2.;
            sizes[ix] = share;
            let at = if place == Place::Before { ix } else { ix + 1 };
            children.insert(at, incoming);
            sizes.insert(at, share);
            return PaneNode::Split {
                dir: *dir,
                children,
                sizes,
            };
        }
        PaneNode::Split {
            dir: *dir,
            children: children
                .iter()
                .map(|c| c.insert_beside(target, incoming.clone(), place))
                .collect(),
            sizes: sizes.clone(),
        }
    }
    fn wrap_beside(
        &self,
        target: i64,
        incoming: PaneNode,
        dir: SplitDir,
        place: Place,
    ) -> PaneNode {
        match self {
            PaneNode::Leaf { id } if *id == target => {
                let children = if place == Place::Before {
                    vec![incoming, self.clone()]
                } else {
                    vec![self.clone(), incoming]
                };
                split(dir, children)
            }
            PaneNode::Leaf { .. } => self.clone(),
            PaneNode::Split {
                dir: own,
                children,
                sizes,
            } => PaneNode::Split {
                dir: *own,
                children: children
                    .iter()
                    .map(|c| c.wrap_beside(target, incoming.clone(), dir, place))
                    .collect(),
                sizes: sizes.clone(),
            },
        }
    }

    /// Drag a leaf onto another pane's edge. Same-axis siblings swap order
    /// and keep their sizes; otherwise the leaf moves beside the target.
    pub fn move_pane(&self, from: i64, to: i64, edge: Edge) -> PaneNode {
        if from == to {
            return self.clone();
        }
        let (Some(from_at), Some(to_at)) = (self.leaf_parent(from), self.leaf_parent(to)) else {
            return self.clone();
        };
        let (dir, place) = edge_split(edge);
        if to_at.2 == dir && from_at.0 == to_at.0 {
            let mut next = self.clone();
            if let PaneNode::Split {
                children, sizes, ..
            } = next.at_path(&from_at.0)
            {
                let (from_ix, n) = (from_at.1, children.len());
                let mut at = if place == Place::After {
                    to_at.1 + 1
                } else {
                    to_at.1
                }
                .min(n);
                if from_ix < at {
                    at -= 1;
                }
                if from_ix != at {
                    let child = children.remove(from_ix);
                    let size = sizes.remove(from_ix);
                    children.insert(at, child);
                    sizes.insert(at, size);
                }
            }
            return next;
        }
        let Some(tree) = self.remove(from) else {
            return self.clone();
        };
        if !tree.contains(to) {
            return self.clone();
        }
        tree.place_layout(leaf(from), to, edge)
    }

    /// Open `id` on `to`'s edge, or move it there when already present.
    pub fn place_pane(&self, id: i64, to: i64, edge: Edge) -> PaneNode {
        if id == to || !self.contains(to) {
            return self.clone();
        }
        if self.contains(id) {
            return self.move_pane(id, to, edge);
        }
        self.place_layout(leaf(id), to, edge)
    }

    /// Place an intact tree beside one pane.
    pub fn place_layout(&self, incoming: PaneNode, to: i64, edge: Edge) -> PaneNode {
        if !self.contains(to) {
            return self.clone();
        }
        let (dir, place) = edge_split(edge);
        match self.leaf_parent(to) {
            Some((_, _, parent)) if parent == dir => self.insert_beside(to, incoming, place),
            _ => self.wrap_beside(to, incoming, dir, place),
        }
    }

    /// Drop unknown or repeated leaves, as restore does. None when empty.
    pub fn retain(&self, keep: &mut impl FnMut(i64) -> bool) -> Option<PaneNode> {
        match self {
            PaneNode::Leaf { id } => keep(*id).then(|| self.clone()),
            PaneNode::Split {
                dir,
                children,
                sizes,
            } => {
                if children.len() != sizes.len() {
                    let ids = self.leaf_ids();
                    let kept: Vec<_> = ids.into_iter().filter(|id| keep(*id)).map(leaf).collect();
                    return match kept.len() {
                        0 => None,
                        1 => kept.into_iter().next(),
                        _ => Some(split(*dir, kept)),
                    };
                }
                let kept: Vec<_> = children
                    .iter()
                    .zip(sizes)
                    .filter_map(|(c, s)| c.retain(keep).map(|c| (c, *s)))
                    .collect();
                match kept.len() {
                    0 => None,
                    1 => kept.into_iter().next().map(|(c, _)| c),
                    _ => {
                        let sizes: Vec<_> = kept.iter().map(|(_, s)| *s).collect();
                        Some(PaneNode::Split {
                            dir: *dir,
                            children: kept.into_iter().map(|(c, _)| c).collect(),
                            sizes: normalize(&sizes),
                        })
                    }
                }
            }
        }
    }
}

pub(crate) fn sizes_at_boundary(
    current: &[f32],
    index: usize,
    boundary: f32,
    min_size: f32,
) -> Vec<f32> {
    if index + 1 >= current.len() {
        return current.to_vec();
    }
    let mut sizes = current.to_vec();
    let before: f32 = sizes[..index].iter().sum();
    let pair = sizes[index] + sizes[index + 1];
    let min = min_size.min(pair / 2.);
    let first = (boundary - before).max(min).min(pair - min);
    sizes[index] = first;
    sizes[index + 1] = pair - first;
    sizes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(node: &PaneNode) -> Vec<i64> {
        node.leaf_ids()
    }
    fn rects(node: &PaneNode) -> Vec<[f32; 4]> {
        node.leaves()
            .iter()
            .map(|l| [l.rect.x, l.rect.y, l.rect.w, l.rect.h])
            .collect()
    }
    fn close(a: &[[f32; 4]], b: &[[f32; 4]]) {
        assert_eq!(a.len(), b.len());
        for (a, b) in a.iter().zip(b) {
            for (x, y) in a.iter().zip(b) {
                assert!((x - y).abs() < 1e-5, "{a:?} != {b:?}");
            }
        }
    }

    #[test]
    fn sash_sizes_move_only_adjacent_panes_and_clamp() {
        let r = sizes_at_boundary(&[0.2, 0.3, 0.5], 1, 0.7, MIN_SIZE);
        assert!((r[0] - 0.2).abs() < 1e-5 && (r[1] - 0.5).abs() < 1e-5);
        assert!((r[2] - 0.3).abs() < 1e-5);
        let r = sizes_at_boundary(&[0.5, 0.5], 0, 0.99, MIN_SIZE);
        assert!((r[0] - 0.92).abs() < 1e-5);
        let r = sizes_at_boundary(&[0.5, 0.5], 0, 0.01, MIN_SIZE);
        assert!((r[0] - 0.08).abs() < 1e-5);
        assert_eq!(
            sizes_at_boundary(&[0.5, 0.5], 2, 0.5, MIN_SIZE),
            vec![0.5, 0.5]
        );
    }

    #[test]
    fn splits_share_a_group_and_report_their_sash() {
        let tree = leaf(1).split_pane(1, SplitDir::Right, 2);
        close(&rects(&tree), &[[0., 0., 0.5, 1.], [0.5, 0., 0.5, 1.]]);
        let sashes = tree.sashes();
        assert_eq!(sashes.len(), 1);
        assert_eq!((sashes[0].index, sashes[0].dir), (0, SplitDir::Right));
        assert_eq!(sashes[0].group, Rect::FULL);
        let three = tree.split_pane(2, SplitDir::Right, 3);
        close(
            &rects(&three),
            &[
                [0., 0., 1. / 3., 1.],
                [1. / 3., 0., 1. / 3., 1.],
                [2. / 3., 0., 1. / 3., 1.],
            ],
        );
        let nested = tree.split_pane(2, SplitDir::Down, 3);
        close(
            &rects(&nested),
            &[[0., 0., 0.5, 1.], [0.5, 0., 0.5, 0.5], [0.5, 0.5, 0.5, 0.5]],
        );
        let mut resized = nested.clone();
        resized.place_sash(&[1], 0, 0.25);
        close(
            &rects(&resized),
            &[
                [0., 0., 0.5, 1.],
                [0.5, 0., 0.5, 0.25],
                [0.5, 0.25, 0.5, 0.75],
            ],
        );
    }

    #[test]
    fn edges_come_from_the_pane_centre() {
        let r = Rect {
            x: 0.,
            y: 0.,
            w: 100.,
            h: 100.,
        };
        assert_eq!(edge_from_point(20., 50., r), Edge::Left);
        assert_eq!(edge_from_point(80., 50., r), Edge::Right);
        assert_eq!(edge_from_point(50., 20., r), Edge::Top);
        assert_eq!(edge_from_point(50., 80., r), Edge::Bottom);
    }

    #[test]
    fn move_pane_reorders_stacks_and_nests() {
        let row = leaf(1).split_pane(1, SplitDir::Right, 2);
        let next = row.move_pane(1, 2, Edge::Right);
        assert_eq!(ids(&next), [2, 1]);
        close(&rects(&next), &[[0., 0., 0.5, 1.], [0.5, 0., 0.5, 1.]]);

        let next = row.move_pane(2, 1, Edge::Bottom);
        assert_eq!(ids(&next), [1, 2]);
        close(&rects(&next), &[[0., 0., 1., 0.5], [0., 0.5, 1., 0.5]]);

        let three = row.split_pane(2, SplitDir::Right, 3);
        let next = three.move_pane(3, 1, Edge::Bottom);
        assert_eq!(ids(&next), [1, 3, 2]);
        close(
            &rects(&next),
            &[[0., 0., 0.5, 0.5], [0., 0.5, 0.5, 0.5], [0.5, 0., 0.5, 1.]],
        );

        let stacked = leaf(1).split_pane(1, SplitDir::Down, 2);
        let mixed = stacked.split_pane(1, SplitDir::Right, 3);
        let next = mixed.move_pane(3, 1, Edge::Bottom);
        assert_eq!(ids(&next), [1, 3, 2]);
        close(
            &rects(&next),
            &[[0., 0., 1., 0.25], [0., 0.25, 1., 0.25], [0., 0.5, 1., 0.5]],
        );
    }

    #[test]
    fn place_pane_splits_inserts_moves_and_ignores_bad_targets() {
        let right = leaf(1).place_pane(2, 1, Edge::Right);
        assert_eq!(ids(&right), [1, 2]);
        assert_eq!(ids(&leaf(1).place_pane(2, 1, Edge::Left)), [2, 1]);
        let below = leaf(1).place_pane(2, 1, Edge::Bottom);
        close(&rects(&below)[1..], &[[0., 0.5, 1., 0.5]]);

        let row = leaf(1).split_pane(1, SplitDir::Right, 2);
        assert_eq!(ids(&row.place_pane(3, 1, Edge::Right)), [1, 3, 2]);
        let nested = row.place_pane(3, 1, Edge::Bottom);
        assert_eq!(ids(&nested), [1, 3, 2]);
        close(
            &rects(&nested),
            &[[0., 0., 0.5, 0.5], [0., 0.5, 0.5, 0.5], [0.5, 0., 0.5, 1.]],
        );
        assert_eq!(ids(&row.place_pane(1, 2, Edge::Right)), [2, 1]);
        assert_eq!(leaf(1).place_pane(2, 9, Edge::Right), leaf(1));
        assert_eq!(leaf(1).place_pane(1, 1, Edge::Right), leaf(1));
    }

    #[test]
    fn removing_collapses_and_focus_moves_to_a_neighbour() {
        let tree = leaf(1)
            .split_pane(1, SplitDir::Right, 2)
            .split_pane(2, SplitDir::Down, 3);
        assert_eq!(tree.sibling(3), Some(2));
        assert_eq!(tree.sibling(1), Some(2));
        let tree = tree.remove(2).unwrap();
        assert_eq!(ids(&tree), [1, 3]);
        close(&rects(&tree), &[[0., 0., 0.5, 1.], [0.5, 0., 0.5, 1.]]);
        assert_eq!(tree.remove(3).unwrap(), leaf(1));
        assert_eq!(leaf(1).remove(1), None);
    }

    #[test]
    fn neighbours_prefer_shared_edges() {
        let tree = leaf(1)
            .split_pane(1, SplitDir::Right, 2)
            .split_pane(2, SplitDir::Down, 3);
        assert_eq!(tree.neighbor(1, Edge::Right), Some(2));
        assert_eq!(tree.neighbor(3, Edge::Left), Some(1));
        assert_eq!(tree.neighbor(2, Edge::Bottom), Some(3));
        assert_eq!(tree.neighbor(3, Edge::Top), Some(2));
        assert_eq!(tree.neighbor(1, Edge::Left), None);
    }

    #[test]
    fn serialized_trees_round_trip_and_restore_prunes_leaves() {
        let tree = leaf(1)
            .split_pane(1, SplitDir::Right, 2)
            .split_pane(2, SplitDir::Down, 3);
        let json = serde_json::to_string(&tree).unwrap();
        assert_eq!(serde_json::from_str::<PaneNode>(&json).unwrap(), tree);
        let pruned = tree.retain(&mut |id| id != 2).unwrap();
        assert_eq!(ids(&pruned), [1, 3]);
        assert_eq!(tree.retain(&mut |id| id == 3).unwrap(), leaf(3));
    }
}
