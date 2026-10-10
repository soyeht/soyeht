//! How a workspace's panes are arranged: the stored split tree of `workspace.layout`, and the
//! operations the desktop client applies to it. The tree is the protocol's own
//! `WorkspacePaneLayoutNode`; nothing here defines a second shape for it.
//!
//! A workspace whose layout is missing, unreadable or out of date shows every live pane: panes
//! the tree no longer has are dropped from it, panes it does not list yet are added beside the
//! rest, and with no usable tree the panes tile in the squarest grid.

use serde_json::Value;
use soyeht_protocol::{
    Pane, PaneMode, WorkspacePaneLayoutAxis as Axis, WorkspacePaneLayoutBranch as Branch,
    WorkspacePaneLayoutEnvelope as Envelope, WorkspacePaneLayoutKind as Kind,
    WorkspacePaneLayoutLeaf as Leaf, WorkspacePaneLayoutMode, WorkspacePaneLayoutNode as Node,
    WorkspacePaneLayoutSplit as Split, WritingKind,
};

/// The envelope version this client writes: the one that carries companion annotations.
const VERSION: u32 = 2;
const DEFAULT_WEIGHT: f64 = 1.0;

/// The layout kind the engine stores for a pane.
pub fn kind_of(pane: &Pane) -> Kind {
    match pane.mode {
        PaneMode::Terminal => Kind::Terminal,
        PaneMode::Agent => Kind::Agent,
        PaneMode::Editor => Kind::Editor,
        PaneMode::Web => Kind::Web,
        PaneMode::Writing => match pane.writing_kind {
            Some(WritingKind::Text) => Kind::Text,
            Some(WritingKind::Presentation) => Kind::Presentation,
            Some(WritingKind::Screenplay) => Kind::Screenplay,
            None => Kind::UnknownWriting,
        },
    }
}

fn leaf(pane: &Pane) -> Node {
    Node::V0(Leaf {
        pane_id: pane.pane_id.clone(),
        kind: kind_of(pane),
    })
}

fn branch(node: Node) -> Branch {
    Branch {
        weight: DEFAULT_WEIGHT,
        node,
    }
}

fn split(axis: Axis, nodes: Vec<Node>) -> Node {
    Node::V1(Split {
        axis,
        children: nodes.into_iter().map(branch).collect(),
        companion: None,
    })
}

/// The arrangement to draw for `panes`, in the order the workspace lists them. `None` when
/// there is nothing to draw.
pub fn resolve(layout: &Value, panes: &[&Pane]) -> Option<Node> {
    if panes.is_empty() {
        return None;
    }
    // A layout that cannot be read falls back to the automatic arrangement, as on the Mac.
    let stored = match serde_json::from_value::<Envelope>(layout.clone()) {
        Ok(envelope) => envelope.tree,
        Err(_) => None,
    };
    let Some(tree) = stored.and_then(|tree| retain(tree, panes)) else {
        return Some(grid(panes));
    };
    let missing: Vec<Node> = panes
        .iter()
        .filter(|pane| !contains(&tree, &pane.pane_id))
        .map(|pane| leaf(pane))
        .collect();
    if missing.is_empty() {
        return Some(tree);
    }
    // Panes the stored tree does not know sit beside it, along the axis the tree already uses.
    let axis = match &tree {
        Node::V1(split) => split.axis.clone(),
        Node::V0(_) => Axis::Horizontal,
    };
    let mut nodes = vec![tree];
    nodes.extend(missing);
    Some(normalize(split(axis, nodes)))
}

/// The squarest grid: rows of `ceil(sqrt(n))` panes.
fn grid(panes: &[&Pane]) -> Node {
    let columns = ((panes.len() as f64).sqrt().ceil() as usize).max(1);
    let mut rows: Vec<Node> = panes
        .chunks(columns)
        .map(|chunk| {
            let leaves: Vec<Node> = chunk.iter().map(|pane| leaf(pane)).collect();
            match <[Node; 1]>::try_from(leaves) {
                Ok([only]) => only,
                Err(leaves) => split(Axis::Horizontal, leaves),
            }
        })
        .collect();
    if rows.len() == 1 {
        return rows.remove(0);
    }
    split(Axis::Vertical, rows)
}

/// The tree without the panes that are gone. A leaf whose kind no longer matches its pane is
/// treated as gone, as on the Mac.
fn retain(node: Node, panes: &[&Pane]) -> Option<Node> {
    match node {
        Node::V0(leaf) => panes
            .iter()
            .any(|pane| pane.pane_id == leaf.pane_id && kind_of(pane) == leaf.kind)
            .then_some(Node::V0(leaf)),
        Node::V1(mut split) => {
            split.children = split
                .children
                .into_iter()
                .filter_map(|branch| {
                    retain(branch.node, panes).map(|node| Branch {
                        weight: branch.weight,
                        node,
                    })
                })
                .collect();
            (!split.children.is_empty()).then_some(normalize(Node::V1(split)))
        }
    }
}

pub fn contains(node: &Node, pane_id: &str) -> bool {
    match node {
        Node::V0(leaf) => leaf.pane_id == pane_id,
        Node::V1(split) => split
            .children
            .iter()
            .any(|branch| contains(&branch.node, pane_id)),
    }
}

/// A split of one child is that child; a child split on the same axis as its parent is merged
/// into it (its weight shared out by the child's own proportions).
fn normalize(node: Node) -> Node {
    let Node::V1(mut parent) = node else {
        return node;
    };
    let mut children = Vec::new();
    for branch in parent.children {
        match normalize(branch.node) {
            Node::V1(inner) if inner.axis == parent.axis && inner.companion.is_none() => {
                let total: f64 = inner.children.iter().map(|child| child.weight).sum();
                for child in inner.children {
                    children.push(Branch {
                        weight: if total > 0.0 {
                            branch.weight * child.weight / total
                        } else {
                            branch.weight
                        },
                        node: child.node,
                    });
                }
            }
            node => children.push(Branch {
                weight: branch.weight,
                node,
            }),
        }
    }
    if children.len() == 1 {
        return children.remove(0).node;
    }
    parent.children = children;
    Node::V1(parent)
}

/// `anchor` with a new pane placed beside it (to its right for `Axis::Horizontal`, below it
/// for `Axis::Vertical`), the two sharing the room the anchor had. `None` when `anchor` is not
/// in the tree.
pub fn split_pane(tree: &Node, anchor: &str, new: &Pane, axis: Axis) -> Option<Node> {
    fn replace(node: &Node, anchor: &str, new: &Node, axis: &Axis) -> Option<Node> {
        match node {
            Node::V0(leaf) if leaf.pane_id == anchor => {
                Some(split(axis.clone(), vec![node.clone(), new.clone()]))
            }
            Node::V0(_) => None,
            Node::V1(parent) => {
                for (index, child) in parent.children.iter().enumerate() {
                    if let Some(replaced) = replace(&child.node, anchor, new, axis) {
                        let mut parent = parent.clone();
                        parent.children[index].node = replaced;
                        return Some(Node::V1(parent));
                    }
                }
                None
            }
        }
    }
    replace(tree, anchor, &leaf(new), &axis).map(normalize)
}

/// The envelope to store: an arrangement the person chose, so it is `custom`.
pub fn envelope(tree: Node) -> Envelope {
    Envelope {
        version: VERSION,
        mode: WorkspacePaneLayoutMode::Custom,
        tree: Some(tree),
    }
}

/// The panes of a tree, left to right and top to bottom.
pub fn pane_ids(node: &Node) -> Vec<&str> {
    match node {
        Node::V0(leaf) => vec![leaf.pane_id.as_str()],
        Node::V1(split) => split
            .children
            .iter()
            .flat_map(|branch| pane_ids(&branch.node))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::pane;
    use serde_json::json;

    fn panes(ids: &[&str]) -> Vec<Pane> {
        ids.iter().map(|id| pane(id, "w", "terminal")).collect()
    }

    fn refs(panes: &[Pane]) -> Vec<&Pane> {
        panes.iter().collect()
    }

    fn stored(tree: Value) -> Value {
        json!({"version": 2, "mode": "custom", "tree": tree})
    }

    fn leaf_json(id: &str) -> Value {
        json!({"paneId": id, "kind": "terminal"})
    }

    #[test]
    fn no_panes_means_nothing_to_draw() {
        assert!(resolve(&json!({}), &[]).is_none());
    }

    #[test]
    fn without_a_stored_tree_panes_tile_in_the_squarest_grid() {
        let panes = panes(&["a", "b", "c", "d", "e"]);
        let tree = resolve(&json!({}), &refs(&panes)).unwrap();
        assert_eq!(pane_ids(&tree), ["a", "b", "c", "d", "e"]);
        let Node::V1(rows) = tree else {
            panic!("five panes need rows")
        };
        assert_eq!(rows.axis, Axis::Vertical);
        assert_eq!(rows.children.len(), 2);
        let one = panes[..1].iter().collect::<Vec<_>>();
        assert!(matches!(resolve(&json!({}), &one), Some(Node::V0(_))));
    }

    #[test]
    fn a_stored_tree_is_kept_and_its_weights_are_honoured() {
        let panes = panes(&["a", "b"]);
        let layout = stored(json!({"axis": "vertical", "children": [
            {"weight": 3.0, "node": leaf_json("a")}, {"weight": 1.0, "node": leaf_json("b")}
        ]}));
        let Some(Node::V1(root)) = resolve(&layout, &refs(&panes)) else {
            panic!("expected a split")
        };
        assert_eq!(root.axis, Axis::Vertical);
        assert_eq!(root.children[0].weight, 3.0);
    }

    #[test]
    fn closed_panes_leave_the_stored_tree_and_unknown_panes_join_it() {
        let live = panes(&["a", "c"]);
        let layout = stored(json!({"axis": "horizontal", "children": [
            {"weight": 1.0, "node": leaf_json("a")}, {"weight": 1.0, "node": leaf_json("gone")}
        ]}));
        let tree = resolve(&layout, &refs(&live)).unwrap();
        assert_eq!(pane_ids(&tree), ["a", "c"]);
    }

    #[test]
    fn a_leaf_of_the_wrong_kind_counts_as_gone() {
        let live = vec![pane("a", "w", "agent")];
        let layout = stored(leaf_json("a"));
        let tree = resolve(&layout, &refs(&live)).unwrap();
        let Node::V0(only) = tree else {
            panic!("one pane is a leaf")
        };
        assert_eq!(only.kind, Kind::Agent);
    }

    #[test]
    fn splitting_places_the_new_pane_beside_the_anchor() {
        let panes = panes(&["a", "b"]);
        let tree = resolve(&json!({}), &refs(&panes)).unwrap();
        let fresh = pane("n", "w", "terminal");
        let tree = split_pane(&tree, "a", &fresh, Axis::Vertical).unwrap();
        assert_eq!(pane_ids(&tree), ["a", "n", "b"]);
        let Node::V1(root) = &tree else {
            panic!("expected a split")
        };
        assert_eq!(root.axis, Axis::Horizontal);
        assert!(matches!(&root.children[0].node, Node::V1(inner) if inner.axis == Axis::Vertical));
        assert!(split_pane(&tree, "missing", &fresh, Axis::Vertical).is_none());
    }

    #[test]
    fn splitting_along_the_parent_axis_flattens_and_shares_the_weight() {
        let panes = panes(&["a", "b"]);
        let tree = resolve(&json!({}), &refs(&panes)).unwrap();
        let fresh = pane("n", "w", "terminal");
        let Some(Node::V1(root)) = split_pane(&tree, "a", &fresh, Axis::Horizontal) else {
            panic!("expected a split")
        };
        assert_eq!(root.children.len(), 3);
        let weights: Vec<f64> = root.children.iter().map(|child| child.weight).collect();
        assert_eq!(weights, [0.5, 0.5, 1.0]);
    }

    #[test]
    fn the_stored_envelope_is_custom_and_round_trips() {
        let panes = panes(&["a", "b"]);
        let tree = resolve(&json!({}), &refs(&panes)).unwrap();
        let value = serde_json::to_value(envelope(tree.clone())).unwrap();
        assert_eq!(value["mode"], "custom");
        assert_eq!(value["version"], 2);
        assert_eq!(resolve(&value, &refs(&panes)), Some(tree));
    }
}
