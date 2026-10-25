//! How a workspace's panes are arranged: the stored split tree of `workspace.layout`, and the
//! operations the desktop client applies to it. The tree is the protocol's own
//! `WorkspacePaneLayoutNode`; nothing here defines a second shape for it.
//!
//! A workspace whose layout is missing, unreadable or out of date shows every live pane: panes
//! the tree no longer has are dropped from it, and panes it does not list yet are added the way
//! the Mac adds them, by role (`arrange`); with no usable tree every pane is added that way.
//!
//! The rules the Mac client applies to a tree (reading a stored layout, removing panes,
//! flattening, splitting, minimum sizes, where a pane sits, where a pane the tree does not list
//! goes) are the cases of `contracts/vectors/pane-layout.json`, which both clients pass.

mod arrange;
mod dock;
mod resize;

pub use arrange::Sizing;
pub use dock::{EDGE_FRACTION, Rect, Zone, move_pane, zone_at, zone_rect};
pub use resize::{Refused, equalize, resize};
use serde_json::Value;
use soyeht_protocol::{
    Pane, PaneMode, WorkspacePaneLayoutAxis as Axis, WorkspacePaneLayoutBranch as Branch,
    WorkspacePaneLayoutCompanion as Companion, WorkspacePaneLayoutEnvelope as Envelope,
    WorkspacePaneLayoutKind as Kind, WorkspacePaneLayoutLeaf as Leaf, WorkspacePaneLayoutMode,
    WorkspacePaneLayoutNode as Node, WorkspacePaneLayoutSplit as Split, WritingKind,
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
pub fn resolve(layout: &Value, panes: &[&Pane], sizing: &Sizing) -> Option<Node> {
    let (mut tree, custom) = match stored_tree(layout, panes) {
        Some((tree, mode)) => (Some(tree), mode == WorkspacePaneLayoutMode::Custom),
        None => (None, false),
    };
    for pane in panes {
        tree = Some(arrange::insert(tree, pane, !custom, sizing));
    }
    tree
}

/// The stored arrangement of `layout` for `panes`, as the Mac reads it (`PaneLayoutState`): a
/// layout that is missing, of an unknown version or malformed gives `None`; panes that are gone
/// leave the tree; and when a pane that stays is of another kind than its leaf says, the whole
/// arrangement is discarded.
fn stored_tree(layout: &Value, panes: &[&Pane]) -> Option<(Node, WorkspacePaneLayoutMode)> {
    // A layout that cannot be read falls back to the automatic arrangement, as on the Mac.
    let Ok(envelope) = serde_json::from_value::<Envelope>(layout.clone()) else {
        return None;
    };
    let mut tree = envelope.tree?;
    match envelope.version {
        VERSION => {}
        // Version 1 predates companion annotations; they are not read from it.
        1 => without_companions(&mut tree),
        _ => return None,
    }
    if !is_valid(&tree) {
        return None;
    }
    tree = arrange::adopt_live_editor(tree, panes);
    let gone: Vec<String> = pane_ids(&tree)
        .into_iter()
        .filter(|id| !panes.iter().any(|pane| pane.pane_id == *id))
        .map(str::to_owned)
        .collect();
    for id in gone {
        tree = remove_pane(tree, &id)?;
    }
    let all_match = panes_of(&tree).into_iter().all(|leaf| {
        panes
            .iter()
            .any(|pane| pane.pane_id == leaf.pane_id && kind_of(pane) == leaf.kind)
    });
    all_match.then_some((tree, envelope.mode))
}

fn without_companions(node: &mut Node) {
    if let Node::V1(split) = node {
        split.companion = None;
        for branch in &mut split.children {
            without_companions(&mut branch.node);
        }
    }
}

fn panes_of(node: &Node) -> Vec<&Leaf> {
    match node {
        Node::V0(leaf) => vec![leaf],
        Node::V1(split) => split
            .children
            .iter()
            .flat_map(|branch| panes_of(&branch.node))
            .collect(),
    }
}

/// The rules the Mac checks before it uses a stored tree: a split has two or more children with
/// positive finite weights, every pane appears once, and a companion pairs a writing pane with
/// an agent pane that are the only two children of its split.
fn is_valid(root: &Node) -> bool {
    let leaves = panes_of(root);
    let unique = leaves.iter().enumerate().all(|(index, leaf)| {
        leaves[..index]
            .iter()
            .all(|seen| seen.pane_id != leaf.pane_id)
    });
    unique && leaves.iter().all(|leaf| !leaf.pane_id.is_empty()) && sound(root, &mut Vec::new())
}

fn sound(node: &Node, paired: &mut Vec<String>) -> bool {
    let Node::V1(split) = node else {
        return true;
    };
    let weights_ok = split
        .children
        .iter()
        .all(|branch| branch.weight.is_finite() && branch.weight > 0.0);
    if split.children.len() < 2 || !weights_ok {
        return false;
    }
    if !split
        .children
        .iter()
        .all(|branch| sound(&branch.node, paired))
    {
        return false;
    }
    split
        .companion
        .as_ref()
        .is_none_or(|companion| companion_ok(split, companion, paired))
}

fn companion_ok(split: &Split, companion: &Companion, paired: &mut Vec<String>) -> bool {
    let members = [&companion.writing_pane_id, &companion.agent_pane_id];
    let direct: Vec<&Leaf> = split
        .children
        .iter()
        .filter_map(|branch| match &branch.node {
            Node::V0(leaf) => Some(leaf),
            Node::V1(_) => None,
        })
        .collect();
    let writing = direct
        .iter()
        .find(|leaf| &leaf.pane_id == members[0])
        .map(|leaf| &leaf.kind);
    let agent = direct
        .iter()
        .find(|leaf| &leaf.pane_id == members[1])
        .map(|leaf| &leaf.kind);
    let fresh = members.iter().all(|id| !paired.contains(id));
    let ok = members[0] != members[1]
        && split.children.len() == 2
        && direct.len() == 2
        && fresh
        && matches!(
            writing,
            Some(Kind::Text | Kind::Presentation | Kind::Screenplay)
        )
        && matches!(agent, Some(Kind::Agent));
    if ok {
        paired.extend(members.map(Clone::clone));
    }
    ok
}

/// `node` without pane `pane_id`; a split left with one child is that child. `None` when
/// nothing remains.
fn remove_pane(node: Node, pane_id: &str) -> Option<Node> {
    match node {
        Node::V0(leaf) => (leaf.pane_id != pane_id).then_some(Node::V0(leaf)),
        Node::V1(mut split) => {
            split.children = split
                .children
                .into_iter()
                .filter_map(|branch| {
                    remove_pane(branch.node, pane_id).map(|node| Branch {
                        weight: branch.weight,
                        node,
                    })
                })
                .collect();
            if split.children.len() < 2 {
                return split.children.pop().map(|only| only.node);
            }
            // A companion survives only while both of its panes are direct children.
            if !keeps_companion(&split) {
                split.companion = None;
            }
            Some(normalize(Node::V1(split)))
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

/// Whether a split still pairs its companion's two panes: they are its only two children.
fn keeps_companion(split: &Split) -> bool {
    let Some(companion) = &split.companion else {
        return false;
    };
    let direct: Vec<&str> = split
        .children
        .iter()
        .filter_map(|branch| match &branch.node {
            Node::V0(leaf) => Some(leaf.pane_id.as_str()),
            Node::V1(_) => None,
        })
        .collect();
    split.children.len() == 2
        && direct.len() == 2
        && direct.contains(&companion.writing_pane_id.as_str())
        && direct.contains(&companion.agent_pane_id.as_str())
}

/// A split of one child is that child; a child split on the same axis as its parent is merged
/// into it (its weight shared out by the child's own proportions). A split that carries a
/// companion keeps its children as they are, and loses the companion when they no longer pair
/// its two panes.
fn normalize(node: Node) -> Node {
    let Node::V1(mut parent) = node else {
        return node;
    };
    if parent.companion.is_some() {
        parent.children = parent
            .children
            .into_iter()
            .map(|branch| Branch {
                weight: branch.weight,
                node: normalize(branch.node),
            })
            .collect();
        if keeps_companion(&parent) {
            return Node::V1(parent);
        }
        parent.companion = None;
        return normalize(Node::V1(parent));
    }
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

/// `node` with the pane `pane_id` replaced by what `with` makes of it. `None` when the pane is
/// not in the tree. A split keeps its axis, its weights and its companion.
fn replace_leaf(node: &Node, pane_id: &str, with: &impl Fn(&Node) -> Node) -> Option<Node> {
    match node {
        Node::V0(leaf) if leaf.pane_id == pane_id => Some(with(node)),
        Node::V0(_) => None,
        Node::V1(parent) => parent
            .children
            .iter()
            .enumerate()
            .find_map(|(index, child)| {
                let replaced = replace_leaf(&child.node, pane_id, with)?;
                let mut parent = parent.clone();
                parent.children[index].node = replaced;
                Some(Node::V1(parent))
            }),
    }
}

/// `anchor` with a new pane placed beside it (to its right for `Axis::Horizontal`, below it
/// for `Axis::Vertical`), the two sharing the room the anchor had. `None` when `anchor` is not
/// in the tree or the new pane is.
pub fn split_pane(tree: &Node, anchor: &str, new: &Pane, axis: Axis) -> Option<Node> {
    if contains(tree, &new.pane_id) {
        return None;
    }
    let new = leaf(new);
    replace_leaf(tree, anchor, &|anchor| {
        split(axis.clone(), vec![anchor.clone(), new.clone()])
    })
    .map(normalize)
}

/// The pair a writing pane and the agent beside it make: `writing` replaced by a split of it and
/// `agent`, which carries the companion annotation and so is never merged into its parent. `None`
/// when `writing` is not a writing pane of the tree, `agent` is not an agent pane, or `agent` is
/// in the tree already, or `writing` has a companion already.
pub fn split_companion(tree: &Node, writing: &str, agent: &Pane, axis: Axis) -> Option<Node> {
    if kind_of(agent) != Kind::Agent
        || contains(tree, &agent.pane_id)
        || companion_of(tree, writing).is_some()
    {
        return None;
    }
    let anchor = find_leaf(tree, writing).filter(|anchor| is_writing(&anchor.kind))?;
    let pair = Node::V1(Split {
        axis,
        children: vec![branch(Node::V0(anchor)), branch(leaf(agent))],
        companion: Some(Companion {
            writing_pane_id: writing.to_owned(),
            agent_pane_id: agent.pane_id.clone(),
        }),
    });
    replace_leaf(tree, writing, &|_| pair.clone()).map(normalize)
}

/// The pair a writing pane belongs to, when it belongs to one.
pub fn companion_of<'a>(node: &'a Node, writing: &str) -> Option<&'a Companion> {
    let Node::V1(split) = node else {
        return None;
    };
    split
        .companion
        .as_ref()
        .filter(|companion| companion.writing_pane_id == writing)
        .or_else(|| {
            split
                .children
                .iter()
                .find_map(|child| companion_of(&child.node, writing))
        })
}

/// Whether a layout kind is one of a document: the kinds a companion agent can sit beside.
fn is_writing(kind: &Kind) -> bool {
    matches!(kind, Kind::Text | Kind::Presentation | Kind::Screenplay)
}

/// The leaf of a pane, when it is in the tree.
fn find_leaf(node: &Node, pane_id: &str) -> Option<Leaf> {
    match node {
        Node::V0(leaf) => (leaf.pane_id == pane_id).then(|| leaf.clone()),
        Node::V1(parent) => parent
            .children
            .iter()
            .find_map(|child| find_leaf(&child.node, pane_id)),
    }
}

/// The envelope to store: an arrangement the person chose, so it is `custom`.
pub fn envelope(tree: Node) -> Envelope {
    Envelope {
        version: VERSION,
        mode: WorkspacePaneLayoutMode::Custom,
        tree: Some(tree),
    }
}

/// Where pane `pane_id` sits along `axis` when every pane is drawn at its floor: its start and
/// its extent. When a tree needs more room than the window has, the panes are drawn at exactly
/// these sizes, so this says how far to scroll to bring the pane into view. A pane stacked in a
/// column is as wide as the column, as on the Mac.
pub fn pane_span(node: &Node, pane_id: &str, sizing: &Sizing, axis: &Axis) -> Option<(f64, f64)> {
    let (width, height) = sizing.minimum(node);
    let extent = if *axis == Axis::Horizontal {
        width
    } else {
        height
    };
    span_in(node, pane_id, sizing, axis, 0.0, extent)
}

fn span_in(
    node: &Node,
    pane_id: &str,
    sizing: &Sizing,
    axis: &Axis,
    start: f64,
    extent: f64,
) -> Option<(f64, f64)> {
    match node {
        Node::V0(leaf) => (leaf.pane_id == pane_id).then_some((start, extent)),
        Node::V1(split) if split.axis == *axis => {
            let gap = sizing.gap(axis);
            let mut offset = start;
            split.children.iter().find_map(|branch| {
                let (width, height) = sizing.minimum(&branch.node);
                let own = if *axis == Axis::Horizontal {
                    width
                } else {
                    height
                };
                let found = span_in(&branch.node, pane_id, sizing, axis, offset, own);
                offset += own + gap;
                found
            })
        }
        Node::V1(split) => split
            .children
            .iter()
            .find_map(|branch| span_in(&branch.node, pane_id, sizing, axis, start, extent)),
    }
}

/// The scroll offset that brings the span `start..start + width` into a viewport of
/// `viewport` points that now shows `scrolled..scrolled + viewport`: unchanged when it is
/// already inside, else the least move that shows it (its left edge first when it is wider).
pub fn reveal_offset(start: f64, width: f64, scrolled: f64, viewport: f64) -> f64 {
    if start < scrolled || width > viewport {
        start
    } else if start + width > scrolled + viewport {
        start + width - viewport
    } else {
        scrolled
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
mod vectors;

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

    /// The floors of the vector: every pane 300 wide and 180 high, 18 apart.
    fn floors() -> Sizing {
        Sizing {
            floor: (300.0, 180.0),
            horizontal_gap: 18.0,
            vertical_gap: 18.0,
            ..Sizing::default()
        }
    }

    fn resolve(layout: &Value, panes: &[&Pane]) -> Option<Node> {
        super::resolve(layout, panes, &Sizing::default())
    }

    /// The panes in a row, as the Mac's stored layout of a person's own arrangement has them.
    fn row(ids: &[&str]) -> Node {
        let children: Vec<Value> = ids
            .iter()
            .map(|id| json!({"weight": 1.0, "node": leaf_json(id)}))
            .collect();
        serde_json::from_value(json!({"axis": "horizontal", "children": children})).unwrap()
    }

    fn stored(tree: Value) -> Value {
        json!({"version": 2, "mode": "custom", "tree": tree})
    }

    fn leaf_json(id: &str) -> Value {
        json!({"paneId": id, "kind": "terminal"})
    }

    /// Pane `a`, then to its right a column of `b` over `c`.
    fn beside_a_stack() -> Node {
        let panes = panes(&["a", "b", "c"]);
        let layout = stored(json!({"axis": "horizontal", "children": [
            {"weight": 1.0, "node": leaf_json("a")},
            {"weight": 1.0, "node": {"axis": "vertical", "children": [
                {"weight": 1.0, "node": leaf_json("b")}, {"weight": 1.0, "node": leaf_json("c")}
            ]}}
        ]}));
        resolve(&layout, &refs(&panes)).unwrap()
    }

    #[test]
    fn no_panes_means_nothing_to_draw() {
        assert!(resolve(&json!({}), &[]).is_none());
    }

    #[test]
    fn without_a_stored_tree_terminals_stack_in_one_column() {
        let panes = panes(&["a", "b", "c", "d", "e"]);
        let tree = resolve(&json!({}), &refs(&panes)).unwrap();
        assert_eq!(pane_ids(&tree), ["a", "b", "c", "d", "e"]);
        let Node::V1(column) = tree else {
            panic!("five panes need a split")
        };
        assert_eq!(column.axis, Axis::Vertical);
        assert_eq!(column.children.len(), 5);
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
    fn a_leaf_of_the_wrong_kind_discards_the_stored_tree() {
        let live = vec![pane("a", "w", "agent"), pane("b", "w", "terminal")];
        let layout = stored(json!({"axis": "horizontal", "children": [
            {"weight": 3.0, "node": leaf_json("a")}, {"weight": 1.0, "node": leaf_json("b")}
        ]}));
        let tree = resolve(&layout, &refs(&live)).unwrap();
        assert_eq!(pane_ids(&tree), ["a", "b"]);
        let Node::V1(root) = tree else {
            panic!("two panes tile")
        };
        assert_eq!(root.children[0].weight, 1.0);
    }

    #[test]
    fn a_single_pane_of_the_wrong_kind_is_drawn_by_its_real_kind() {
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
        let tree = row(&["a", "b"]);
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
        let tree = row(&["a", "b"]);
        let fresh = pane("n", "w", "terminal");
        let Some(Node::V1(root)) = split_pane(&tree, "a", &fresh, Axis::Horizontal) else {
            panic!("expected a split")
        };
        assert_eq!(root.children.len(), 3);
        let weights: Vec<f64> = root.children.iter().map(|child| child.weight).collect();
        assert_eq!(weights, [0.5, 0.5, 1.0]);
    }

    #[test]
    fn a_layout_needs_the_floor_of_every_pane_that_sits_beside_another() {
        let tree = beside_a_stack();
        assert_eq!(floors().minimum(&tree).0, 618.0);
        let one = panes(&["a"]);
        let single = resolve(&json!({}), &refs(&one)).unwrap();
        assert_eq!(floors().minimum(&single).0, 300.0);
    }

    #[test]
    fn a_pane_sits_after_the_panes_beside_it_and_under_none_stacked_above_it() {
        let tree = beside_a_stack();
        let across = |id| pane_span(&tree, id, &floors(), &Axis::Horizontal);
        assert_eq!(across("a"), Some((0.0, 300.0)));
        assert_eq!(across("b"), Some((318.0, 300.0)));
        assert_eq!(across("c"), Some((318.0, 300.0)));
        assert_eq!(across("gone"), None);
        // Down the column the second pane follows the first across the gap.
        let down = |id| pane_span(&tree, id, &floors(), &Axis::Vertical);
        assert_eq!(down("b"), Some((0.0, 180.0)));
        assert_eq!(down("c"), Some((198.0, 180.0)));
        assert_eq!(down("a"), Some((0.0, 378.0)));
    }

    #[test]
    fn revealing_moves_the_view_the_least_that_shows_the_pane() {
        // Inside: no move.
        assert_eq!(reveal_offset(100.0, 300.0, 50.0, 800.0), 50.0);
        // Past the right edge: its right edge meets the viewport's.
        assert_eq!(reveal_offset(600.0, 300.0, 0.0, 800.0), 100.0);
        // Before the left edge: its left edge meets the viewport's.
        assert_eq!(reveal_offset(100.0, 300.0, 400.0, 800.0), 100.0);
        // Wider than the viewport: its left edge wins.
        assert_eq!(reveal_offset(500.0, 900.0, 0.0, 800.0), 500.0);
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
