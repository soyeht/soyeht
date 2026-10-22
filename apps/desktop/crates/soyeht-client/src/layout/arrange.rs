//! Where a pane goes when a workspace's stored tree does not list it, and how a workspace with
//! no tree is arranged: the Mac's rule (`PaneLayoutArrangement`, ADR 0049 P1). Primary panes (the
//! editor, a web page, a writing pane) go left to right; the auxiliary ones (a terminal, an
//! agent) stack in a column on the right. A pane of a role that the tree already has splits the
//! last pane of that role; otherwise it joins the whole tree as a new row (primary, to its left)
//! or a new column (auxiliary, to its right). In the automatic mode every branch then takes the
//! share of the minimum size its subtree needs; an arrangement the person made keeps its shares.

use super::{Axis, Branch, Kind, Node, contains, leaf, normalize};
use soyeht_protocol::Pane;

/// What the weights of an arrangement are measured with: the smallest width and height of a
/// pane (`floor`, or the entry of `kinds` for its kind), the gaps between panes, and the strip a
/// writing companion adds. The default weighs every pane alike.
#[derive(Debug, Clone)]
pub struct Sizing {
    pub floor: (f64, f64),
    pub kinds: Vec<(Kind, (f64, f64))>,
    pub horizontal_gap: f64,
    pub vertical_gap: f64,
    pub companion_strip: f64,
}

impl Default for Sizing {
    fn default() -> Self {
        Self {
            floor: (1.0, 1.0),
            kinds: Vec::new(),
            horizontal_gap: 0.0,
            vertical_gap: 0.0,
            companion_strip: 0.0,
        }
    }
}

impl Sizing {
    fn floor_of(&self, kind: &Kind) -> (f64, f64) {
        self.kinds
            .iter()
            .find(|(known, _)| known == kind)
            .map_or(self.floor, |(_, floor)| *floor)
    }

    /// The smallest width and height `node` can be drawn at.
    pub fn minimum(&self, node: &Node) -> (f64, f64) {
        match node {
            Node::V0(leaf) => self.floor_of(&leaf.kind),
            Node::V1(split) => {
                let sizes: Vec<(f64, f64)> = split
                    .children
                    .iter()
                    .map(|branch| self.minimum(&branch.node))
                    .collect();
                let widest = sizes.iter().map(|size| size.0).fold(0.0, f64::max);
                let tallest = sizes.iter().map(|size| size.1).fold(0.0, f64::max);
                let pair_of_panes = split.children.len() == 2
                    && split
                        .children
                        .iter()
                        .all(|branch| matches!(branch.node, Node::V0(_)));
                // A sizing without a strip draws a companion pair as any other split.
                if split.companion.is_some() && pair_of_panes && self.companion_strip > 0.0 {
                    return (widest, tallest + self.companion_strip);
                }
                let gaps = sizes.len().saturating_sub(1) as f64;
                match split.axis {
                    Axis::Horizontal => (
                        sizes.iter().map(|size| size.0).sum::<f64>() + self.horizontal_gap * gaps,
                        tallest,
                    ),
                    Axis::Vertical => (
                        widest,
                        sizes.iter().map(|size| size.1).sum::<f64>() + self.vertical_gap * gaps,
                    ),
                }
            }
        }
    }

    fn branches(&self, nodes: Vec<Node>, axis: &Axis) -> Vec<Branch> {
        nodes
            .into_iter()
            .map(|node| {
                let (width, height) = self.minimum(&node);
                let weight = if *axis == Axis::Horizontal {
                    width
                } else {
                    height
                };
                Branch { weight, node }
            })
            .collect()
    }

    fn split(&self, axis: Axis, nodes: Vec<Node>) -> Node {
        let children = self.branches(nodes, &axis);
        Node::V1(soyeht_protocol::WorkspacePaneLayoutSplit {
            axis,
            children,
            companion: None,
        })
    }

    fn reweighted(&self, node: Node) -> Node {
        let Node::V1(mut split) = node else {
            return node;
        };
        let nodes = split
            .children
            .into_iter()
            .map(|branch| self.reweighted(branch.node))
            .collect();
        split.children = self.branches(nodes, &split.axis);
        Node::V1(split)
    }
}

fn is_primary(kind: &Kind) -> bool {
    !matches!(kind, Kind::Terminal | Kind::Agent)
}

/// `tree` with the last pane of `primary`'s role replaced by what `transform` makes of it;
/// `None` when the tree has no pane of that role.
fn replace_last(tree: &Node, primary: bool, transform: &dyn Fn(&Node) -> Node) -> Option<Node> {
    match tree {
        Node::V0(only) => (is_primary(&only.kind) == primary).then(|| transform(tree)),
        Node::V1(split) => {
            let mut copy = split.clone();
            for index in (0..copy.children.len()).rev() {
                if let Some(replaced) = replace_last(&copy.children[index].node, primary, transform)
                {
                    copy.children[index].node = replaced;
                    return Some(Node::V1(copy));
                }
            }
            None
        }
    }
}

/// `tree` with `pane` added. `reweigh` is the automatic mode: every branch takes its share
/// afterwards.
pub fn insert(tree: Option<Node>, pane: &Pane, reweigh: bool, sizing: &Sizing) -> Node {
    let added = leaf(pane);
    let Some(tree) = tree else {
        return added;
    };
    if contains(&tree, &pane.pane_id) {
        return tree;
    }
    let primary = matches!(&added, Node::V0(only) if is_primary(&only.kind));
    let axis = if primary {
        Axis::Horizontal
    } else {
        Axis::Vertical
    };
    let inserted = match replace_last(&tree, primary, &|existing| {
        sizing.split(axis.clone(), vec![existing.clone(), added.clone()])
    }) {
        Some(replaced) => normalize(replaced),
        None => {
            let ordered = if primary {
                vec![added, tree]
            } else {
                vec![tree, added]
            };
            normalize(sizing.split(Axis::Horizontal, ordered))
        }
    };
    if reweigh {
        sizing.reweighted(inserted)
    } else {
        inserted
    }
}

/// The stored tree with the single live editor pane in the slot of the first editor leaf whose
/// pane is gone, when the live editor has no leaf yet (the Mac's editor tabs share one slot).
pub fn adopt_live_editor(tree: Node, panes: &[&Pane]) -> Node {
    let mut editors = panes
        .iter()
        .filter(|pane| matches!(super::kind_of(pane), Kind::Editor));
    let (Some(editor), None) = (editors.next(), editors.next()) else {
        return tree;
    };
    if contains(&tree, &editor.pane_id) {
        return tree;
    }
    let gone = super::panes_of(&tree).into_iter().find(|leaf| {
        leaf.kind == Kind::Editor && !panes.iter().any(|pane| pane.pane_id == leaf.pane_id)
    });
    match gone {
        Some(slot) => {
            let slot = slot.pane_id.clone();
            rename(tree, &slot, &editor.pane_id)
        }
        None => tree,
    }
}

fn rename(node: Node, from: &str, to: &str) -> Node {
    match node {
        Node::V0(mut only) => {
            if only.pane_id == from {
                only.pane_id = to.to_owned();
            }
            Node::V0(only)
        }
        Node::V1(mut split) => {
            split.children = split
                .children
                .into_iter()
                .map(|branch| Branch {
                    weight: branch.weight,
                    node: rename(branch.node, from, to),
                })
                .collect();
            Node::V1(split)
        }
    }
}
