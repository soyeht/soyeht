//! Moving a pane to another place in the arrangement by dragging its header: where on a pane
//! a drop lands (`PaneLayoutDropGeometry`) and what the arrangement becomes
//! (`PaneLayoutTreeMutation.moving`). The `dropZone`, `dropZoneRect` and `move` cases of
//! `contracts/vectors/pane-layout.json` hold both clients to the Mac's rules.
//!
//! A pane dropped on an edge of another pane is taken out of the tree and put beside that pane,
//! the two sharing the room it had; dropped in the middle it changes places with that pane.

use super::{
    Axis, Companion, Node, Refused, Split, normalize, panes_of, remove_pane, replace_leaf, split,
};
use std::cmp::Ordering;

/// How much of a pane, measured from each edge, belongs to that edge's zone.
pub const EDGE_FRACTION: f64 = 0.25;

/// Where on a pane a drop lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    Left,
    Right,
    Top,
    Bottom,
    Center,
}

impl Zone {
    /// The axis of the split an edge zone makes: a pane dropped on the left or right of another
    /// goes beside it, on the top or bottom above or below it. The center makes none.
    fn axis(self) -> Option<Axis> {
        match self {
            Self::Left | Self::Right => Some(Axis::Horizontal),
            Self::Top | Self::Bottom => Some(Axis::Vertical),
            Self::Center => None,
        }
    }

    fn goes_before_target(self) -> bool {
        matches!(self, Self::Left | Self::Top)
    }
}

/// A rectangle in points: where a pane is drawn, or one of its zones.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    fn right(&self) -> f64 {
        self.x + self.width.max(0.0)
    }

    fn bottom(&self) -> f64 {
        self.y + self.height.max(0.0)
    }

    fn contains(&self, (x, y): (f64, f64)) -> bool {
        x >= self.x && x <= self.right() && y >= self.y && y <= self.bottom()
    }
}

/// The zone of the pane drawn in `pane` that the point lands on: the edge it is within a quarter
/// of, the nearest when it is within a quarter of two (the left over the right over the top over
/// the bottom when equally near), else the center. `None` outside the pane.
pub fn zone_at(pane: Rect, point: (f64, f64)) -> Option<Zone> {
    if !pane.contains(point) || pane.width <= 0.0 || pane.height <= 0.0 {
        return None;
    }
    let edges = [
        (Zone::Left, (point.0 - pane.x) / pane.width),
        (Zone::Right, (pane.right() - point.0) / pane.width),
        (Zone::Top, (point.1 - pane.y) / pane.height),
        (Zone::Bottom, (pane.bottom() - point.1) / pane.height),
    ];
    // `min_by` keeps the first of equals, and the edges are listed in the order that wins a tie.
    let nearest = edges
        .into_iter()
        .filter(|(_, distance)| *distance <= EDGE_FRACTION)
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal));
    Some(nearest.map_or(Zone::Center, |(zone, _)| zone))
}

/// The part of the pane drawn in `pane` that `zone` covers, which the zone lights up while a
/// pane is over it: a quarter-wide strip along an edge, or what the strips leave in the middle.
pub fn zone_rect(zone: Zone, pane: Rect) -> Rect {
    let (edge_width, edge_height) = (pane.width * EDGE_FRACTION, pane.height * EDGE_FRACTION);
    match zone {
        Zone::Left => Rect {
            width: edge_width,
            ..pane
        },
        Zone::Right => Rect {
            x: pane.right() - edge_width,
            width: edge_width,
            ..pane
        },
        Zone::Top => Rect {
            height: edge_height,
            ..pane
        },
        Zone::Bottom => Rect {
            y: pane.bottom() - edge_height,
            height: edge_height,
            ..pane
        },
        Zone::Center => Rect {
            x: pane.x + edge_width,
            y: pane.y + edge_height,
            width: (pane.width - 2.0 * edge_width).max(0.0),
            height: (pane.height - 2.0 * edge_height).max(0.0),
        },
    }
}

/// `tree` with pane `pane_id` dropped on `zone` of pane `target`. A pane dropped on itself
/// changes nothing. An edge puts the pane beside or above or below the target, in a split of
/// their own that takes the target's place; the two panes of a writing companion only change
/// sides and stay a pair. The center swaps the two panes where they stand.
pub fn move_pane(tree: &Node, pane_id: &str, target: &str, zone: Zone) -> Result<Node, Refused> {
    if pane_id == target {
        return Ok(tree.clone());
    }
    let find = |id: &str| panes_of(tree).into_iter().find(|leaf| leaf.pane_id == id);
    let (Some(source), Some(_)) = (find(pane_id), find(target)) else {
        return Err(Refused::NoSuchPane);
    };
    let Some(axis) = zone.axis() else {
        return Ok(swapped(tree, pane_id, target));
    };
    let before = zone.goes_before_target();
    if let Some(reordered) = reorder_pair(tree, pane_id, target, &axis, before) {
        return Ok(normalize(reordered));
    }
    let moved = Node::V0(source.clone());
    let pruned = remove_pane(tree.clone(), pane_id).ok_or(Refused::NoSuchPane)?;
    replace_leaf(&pruned, target, &|target| {
        let pair = if before {
            [moved.clone(), target.clone()]
        } else {
            [target.clone(), moved.clone()]
        };
        split(axis.clone(), pair.into())
    })
    .map(normalize)
    .ok_or(Refused::NoSuchPane)
}

/// `tree` with two panes exchanged, the branches around them and their weights left as they are.
fn swapped(tree: &Node, first: &str, second: &str) -> Node {
    let leaves = panes_of(tree);
    let pick = |id: &str| leaves.iter().find(|leaf| leaf.pane_id == id).copied();
    let (Some(first_leaf), Some(second_leaf)) = (pick(first), pick(second)) else {
        return tree.clone();
    };
    fn walk(node: &Node, first: &Node, second: &Node, ids: (&str, &str)) -> Node {
        match node {
            Node::V0(leaf) if leaf.pane_id == ids.0 => second.clone(),
            Node::V0(leaf) if leaf.pane_id == ids.1 => first.clone(),
            Node::V0(_) => node.clone(),
            Node::V1(parent) => {
                let mut parent = parent.clone();
                for child in &mut parent.children {
                    child.node = walk(&child.node, first, second, ids);
                }
                Node::V1(parent)
            }
        }
    }
    walk(
        tree,
        &Node::V0(first_leaf.clone()),
        &Node::V0(second_leaf.clone()),
        (first, second),
    )
}

/// The tree with the split that pairs a writing pane and its agent changed to put `pane_id` before
/// or after `target` along `axis`, when those two are the pair. `None` when they are not.
fn reorder_pair(
    tree: &Node,
    pane_id: &str,
    target: &str,
    axis: &Axis,
    before: bool,
) -> Option<Node> {
    let Node::V1(parent) = tree else {
        return None;
    };
    let paired = |companion: &Companion| {
        let has = |id: &str| companion.writing_pane_id == id || companion.agent_pane_id == id;
        has(pane_id) && has(target)
    };
    if parent.companion.as_ref().is_some_and(paired) {
        let direct = |id: &str| {
            parent
                .children
                .iter()
                .find(|branch| matches!(&branch.node, Node::V0(leaf) if leaf.pane_id == id))
        };
        let (moved, anchor) = (direct(pane_id)?, direct(target)?);
        let children = if before {
            vec![moved.clone(), anchor.clone()]
        } else {
            vec![anchor.clone(), moved.clone()]
        };
        return Some(Node::V1(Split {
            axis: axis.clone(),
            children,
            companion: parent.companion.clone(),
        }));
    }
    parent
        .children
        .iter()
        .enumerate()
        .find_map(|(index, child)| {
            let reordered = reorder_pair(&child.node, pane_id, target, axis, before)?;
            let mut parent = parent.clone();
            parent.children[index].node = reordered;
            Some(Node::V1(parent))
        })
}
