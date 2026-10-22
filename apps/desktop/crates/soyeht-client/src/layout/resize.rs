//! Dragging a divider and equalizing a split: the Mac's rules (`PaneLayoutTreeMutation.resizing`
//! and `.equalizing`, with the geometry of `PaneLayoutGeometry`), which the `resize` and
//! `equalize` cases of `contracts/vectors/pane-layout.json` hold both clients to.
//!
//! A split gives each child its floor plus a share of what the room has beyond all the floors
//! and the gaps, by weight. A drag therefore moves the two weights beside the divider by the same
//! amount, so that the pane before it grows by the distance of the drag until a floor stops it.

use super::{Axis, Branch, DEFAULT_WEIGHT, Node, Sizing, Split};

/// Why a tree was not resized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// The distance of the drag is not a number.
    NotFinite,
    /// The path does not lead to a split.
    NotASplit,
    /// The split has no divider there.
    NoSuchDivider,
    /// The weights of the split cannot be shared out.
    BadWeights,
}

/// A split laid out in the room it is given.
struct Placed<'a> {
    split: &'a Split,
    /// Along the split's axis: each child's floor and the room it is given.
    floors: Vec<f64>,
    extents: Vec<f64>,
    /// The gaps between the children.
    gaps: f64,
    /// The room the split's content has: the room it is given, or the floors when they need more.
    content: f64,
}

impl Placed<'_> {
    fn floors_and_gaps(&self) -> f64 {
        self.floors.iter().sum::<f64>() + self.gaps
    }

    /// What the room has beyond the floors and the gaps.
    fn distributable(&self) -> f64 {
        (self.content - self.floors_and_gaps()).max(0.0)
    }

    fn weight_total(&self) -> f64 {
        self.split.children.iter().map(|branch| branch.weight).sum()
    }
}

fn along(size: (f64, f64), axis: &Axis) -> f64 {
    match axis {
        Axis::Horizontal => size.0,
        Axis::Vertical => size.1,
    }
}

fn with_extent(size: (f64, f64), axis: &Axis, extent: f64) -> (f64, f64) {
    match axis {
        Axis::Horizontal => (extent, size.1),
        Axis::Vertical => (size.0, extent),
    }
}

fn lay_out<'a>(split: &'a Split, frame: (f64, f64), sizing: &Sizing) -> Placed<'a> {
    let floors: Vec<f64> = split
        .children
        .iter()
        .map(|branch| along(sizing.minimum(&branch.node), &split.axis))
        .collect();
    let gap = match split.axis {
        Axis::Horizontal => sizing.horizontal_gap,
        Axis::Vertical => sizing.vertical_gap,
    };
    let gaps = gap * split.children.len().saturating_sub(1) as f64;
    let content = along(frame, &split.axis).max(floors.iter().sum::<f64>() + gaps);
    let mut placed = Placed {
        split,
        extents: Vec::new(),
        floors,
        gaps,
        content,
    };
    let (distributable, total) = (placed.distributable(), placed.weight_total());
    placed.extents = placed
        .floors
        .iter()
        .zip(&split.children)
        .map(|(floor, branch)| {
            let share = if total > 0.0 {
                distributable * branch.weight / total
            } else {
                0.0
            };
            floor + share
        })
        .collect();
    placed
}

/// The split at `path`, laid out in a window of `viewport` (width, height).
fn place<'a>(
    tree: &'a Node,
    path: &[usize],
    viewport: (f64, f64),
    sizing: &Sizing,
) -> Result<Placed<'a>, Refused> {
    let mut node = tree;
    let mut path = path;
    let mut frame = (viewport.0.max(0.0), viewport.1.max(0.0));
    loop {
        let Node::V1(split) = node else {
            return Err(Refused::NotASplit);
        };
        let placed = lay_out(split, frame, sizing);
        let Some((first, rest)) = path.split_first() else {
            return Ok(placed);
        };
        let child = split.children.get(*first).ok_or(Refused::NotASplit)?;
        frame = with_extent(frame, &split.axis, placed.extents[*first]);
        node = &child.node;
        path = rest;
    }
}

/// `node` with the split at `path` replaced by `change` of it.
fn updated(node: &Node, path: &[usize], change: &dyn Fn(&Split) -> Split) -> Result<Node, Refused> {
    let Node::V1(split) = node else {
        return Err(Refused::NotASplit);
    };
    let Some((first, rest)) = path.split_first() else {
        return Ok(Node::V1(change(split)));
    };
    let mut copy = split.clone();
    let child = copy.children.get_mut(*first).ok_or(Refused::NotASplit)?;
    child.node = updated(&child.node, rest, change)?;
    Ok(Node::V1(copy))
}

/// `tree` with the divider after child `divider` of the split at `path` moved by `delta` points
/// (positive to the right or down) in a window of `viewport` (width, height). Only the two
/// weights beside the divider change and their sum stays; the floors of what is beside the
/// divider stop the move. A window with no room beyond the floors changes nothing.
pub fn resize(
    tree: &Node,
    path: &[usize],
    divider: usize,
    delta: f64,
    viewport: (f64, f64),
    sizing: &Sizing,
) -> Result<Node, Refused> {
    if !delta.is_finite() {
        return Err(Refused::NotFinite);
    }
    let placed = place(tree, path, viewport, sizing)?;
    let (before, after) = (divider, divider + 1);
    if after >= placed.split.children.len() {
        return Err(Refused::NoSuchDivider);
    }
    let minimum_delta = placed.floors[before] - placed.extents[before];
    let maximum_delta = placed.extents[after] - placed.floors[after];
    let clamped = delta.max(minimum_delta).min(maximum_delta);
    let distributable = placed.distributable();
    let total = placed.weight_total();
    if distributable <= 0.0 {
        return Ok(tree.clone());
    }
    if !total.is_finite() || total <= 0.0 {
        return Err(Refused::BadWeights);
    }
    let requested = clamped * total / distributable;
    let smallest = (total * 1e-9).max(f64::MIN_POSITIVE);
    let (left, right) = (
        placed.split.children[before].weight,
        placed.split.children[after].weight,
    );
    let moved = requested.max(smallest - left).min(right - smallest);
    if !moved.is_finite() || moved == 0.0 {
        return Ok(tree.clone());
    }
    updated(tree, path, &|split| {
        let mut split = split.clone();
        split.children[before].weight += moved;
        split.children[after].weight -= moved;
        split
    })
}

/// `tree` with the children of the split at `path` made equally large where their floors allow
/// it. A floor larger than the equal share wins and the others share the rest.
pub fn equalize(
    tree: &Node,
    path: &[usize],
    viewport: (f64, f64),
    sizing: &Sizing,
) -> Result<Node, Refused> {
    let placed = place(tree, path, viewport, sizing)?;
    let available = (placed.content - placed.gaps).max(0.0);
    let desired = equal_extents(&placed.floors, available);
    let flexible: Vec<f64> = desired
        .iter()
        .zip(&placed.floors)
        .map(|(wanted, floor)| (wanted - floor).max(0.0))
        .collect();
    let distributable: f64 = flexible.iter().sum();
    let smallest = (distributable * 1e-9).max(f64::MIN_POSITIVE);
    let weights: Vec<f64> = if distributable > 0.0 {
        flexible.iter().map(|weight| weight.max(smallest)).collect()
    } else {
        vec![DEFAULT_WEIGHT; flexible.len()]
    };
    updated(tree, path, &|split| Split {
        axis: split.axis.clone(),
        children: split
            .children
            .iter()
            .zip(&weights)
            .map(|(branch, weight)| Branch {
                weight: *weight,
                node: branch.node.clone(),
            })
            .collect(),
        companion: split.companion.clone(),
    })
}

/// The room each child gets when all of `available` is shared equally, except that a child
/// whose floor is larger than the share keeps its floor and the rest share what remains.
fn equal_extents(floors: &[f64], available: f64) -> Vec<f64> {
    let mut desired = floors.to_vec();
    let mut unsettled: Vec<usize> = (0..floors.len()).collect();
    let mut remaining = available;
    while !unsettled.is_empty() {
        let share = remaining / unsettled.len() as f64;
        let (constrained, free): (Vec<usize>, Vec<usize>) =
            unsettled.iter().partition(|index| floors[**index] > share);
        if constrained.is_empty() {
            for index in unsettled {
                desired[index] = share;
            }
            break;
        }
        for index in constrained {
            remaining -= floors[index];
        }
        unsettled = free;
    }
    desired
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{Axis, Branch, Node};
    use soyeht_protocol::{WorkspacePaneLayoutKind as Kind, WorkspacePaneLayoutLeaf as Leaf};

    fn leaf(id: &str) -> Node {
        Node::V0(Leaf {
            pane_id: id.to_owned(),
            kind: Kind::Terminal,
        })
    }

    fn row(weights: &[f64]) -> Node {
        Node::V1(Split {
            axis: Axis::Horizontal,
            children: weights
                .iter()
                .enumerate()
                .map(|(index, weight)| Branch {
                    weight: *weight,
                    node: leaf(&index.to_string()),
                })
                .collect(),
            companion: None,
        })
    }

    fn sizing() -> Sizing {
        Sizing {
            floor: (300.0, 180.0),
            horizontal_gap: 18.0,
            vertical_gap: 18.0,
            ..Sizing::default()
        }
    }

    fn widths(tree: &Node, room: f64) -> Vec<f64> {
        let Node::V1(split) = tree else {
            return Vec::new();
        };
        lay_out(split, (room, 600.0), &sizing()).extents
    }

    #[test]
    fn the_pane_before_the_divider_grows_by_the_distance_of_the_drag() {
        let tree = row(&[1.0, 2.0, 1.0]);
        let before = widths(&tree, 1500.0);
        let moved = resize(&tree, &[], 0, 75.0, (1500.0, 600.0), &sizing()).unwrap();
        let after = widths(&moved, 1500.0);
        assert!((after[0] - before[0] - 75.0).abs() < 1e-9);
        assert!((before[1] - after[1] - 75.0).abs() < 1e-9);
        assert!((after[2] - before[2]).abs() < 1e-9);
    }

    #[test]
    fn a_drag_in_steps_from_the_start_equals_one_drag() {
        // The view resizes the tree the drag started from by the whole distance every time.
        let tree = row(&[1.0, 1.0]);
        let once = resize(&tree, &[], 0, 120.0, (1000.0, 600.0), &sizing()).unwrap();
        let again = resize(&tree, &[], 0, 120.0, (1000.0, 600.0), &sizing()).unwrap();
        assert_eq!(once, again);
        assert_ne!(once, tree);
    }

    #[test]
    fn a_distance_that_is_not_a_number_is_refused() {
        let tree = row(&[1.0, 1.0]);
        for delta in [f64::NAN, f64::INFINITY] {
            assert_eq!(
                resize(&tree, &[], 0, delta, (1000.0, 600.0), &sizing()),
                Err(Refused::NotFinite)
            );
        }
    }

    #[test]
    fn weights_that_cannot_be_shared_are_refused() {
        let tree = row(&[0.0, 0.0]);
        assert_eq!(
            resize(&tree, &[], 0, 10.0, (1000.0, 600.0), &sizing()),
            Err(Refused::BadWeights)
        );
    }
}
