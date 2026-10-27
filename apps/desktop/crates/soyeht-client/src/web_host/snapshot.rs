//! A snapshot of the page for an agent: what the page's reader script returned, kept to the
//! contract's byte budget, with the parent of every node one that is also in the snapshot.

use super::{HostError, Outcome, Page, clip, picture, script};
use serde::Deserialize;
use serde_json::{Value, json};
use soyeht_protocol::{WebSnapshotNode, limits};
use std::collections::HashSet;

/// What the engine's frame cap leaves for the nodes: the budget of the contract less the
/// envelope of the result, as the Mac client keeps it.
const NODE_BUDGET: usize = limits::WEB_SNAPSHOT_BYTES - limits::WEB_PART_BYTES;

#[derive(Deserialize)]
struct Capture {
    nodes: Vec<WebSnapshotNode>,
    truncated: bool,
}

pub(super) fn take(page: &dyn Page, pane_id: &str, args: &Value) -> Result<Outcome, HostError> {
    let generation = page.generation();
    let answer = page.run_script(&script::snapshot_call(generation))?;
    let capture: Capture = serde_json::from_str(&answer)
        .map_err(|_| HostError::Internal("The page returned an invalid result."))?;
    let info = page.info();
    let (nodes, truncated) = bounded(capture);
    let mut value = json!({
        "navigationGeneration": generation.to_string(),
        "title": clip(&info.title, limits::WEB_TITLE_BYTES),
        "url": clip(&info.url, limits::WEB_FIELD_BYTES),
        "nodes": nodes,
        "truncated": truncated,
    });
    let mut attachment = None;
    if super::flag(args, "screenshot") {
        let (reference, bytes) = picture::screenshot(&page.picture()?)?;
        value["screenshot"] = json!(reference);
        attachment = Some((reference, bytes));
    }
    let mut outcome = Outcome::ok(pane_id, value);
    outcome.attachment = attachment;
    Ok(outcome)
}

/// The nodes that fit the budget, in order, and whether any were left out.
fn bounded(capture: Capture) -> (Vec<WebSnapshotNode>, bool) {
    let mut kept: Vec<WebSnapshotNode> = Vec::new();
    let mut accepted: HashSet<String> = HashSet::new();
    let mut used = 2;
    let mut truncated = capture.truncated;
    for mut node in capture.nodes {
        if node
            .parent_ref
            .as_ref()
            .is_some_and(|parent| !accepted.contains(parent))
        {
            node.parent_ref = None;
        }
        let Ok(encoded) = serde_json::to_vec(&node) else {
            continue;
        };
        let size = encoded.len() + usize::from(!kept.is_empty());
        if used + size > NODE_BUDGET {
            truncated = true;
            break;
        }
        used += size;
        accepted.insert(node.r#ref.clone());
        kept.push(node);
    }
    (kept, truncated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use soyeht_protocol::WebRect;

    fn node(index: usize, parent: Option<&str>, name: &str) -> WebSnapshotNode {
        WebSnapshotNode {
            r#ref: format!("e{index}"),
            role: "text".into(),
            name: name.into(),
            state: String::new(),
            value: None,
            rect: WebRect {
                x: 0,
                y: 0,
                width: 10,
                height: 10,
            },
            parent_ref: parent.map(str::to_owned),
        }
    }

    #[test]
    fn a_parent_that_did_not_make_it_into_the_snapshot_is_dropped() {
        let capture = Capture {
            nodes: vec![
                node(1, None, "a"),
                node(2, Some("e1"), "b"),
                node(3, Some("e9"), "c"),
            ],
            truncated: false,
        };
        let (nodes, truncated) = bounded(capture);
        assert!(!truncated);
        assert_eq!(nodes[1].parent_ref.as_deref(), Some("e1"));
        assert_eq!(nodes[2].parent_ref, None);
    }

    #[test]
    fn a_long_page_is_cut_at_the_budget_and_says_so() {
        let name = "x".repeat(limits::WEB_LABEL_BYTES);
        let capture = Capture {
            nodes: (0..200).map(|index| node(index, None, &name)).collect(),
            truncated: false,
        };
        let (nodes, truncated) = bounded(capture);
        assert!(truncated);
        let size = serde_json::to_vec(&nodes).unwrap().len();
        assert!(size <= NODE_BUDGET, "{size}");
        assert!(nodes.len() > 50 && nodes.len() < 200, "{}", nodes.len());
    }

    #[test]
    fn a_page_that_fits_keeps_every_node_and_the_pages_own_flag() {
        let capture = Capture {
            nodes: vec![node(1, None, "a")],
            truncated: true,
        };
        let (nodes, truncated) = bounded(capture);
        assert_eq!(nodes.len(), 1);
        assert!(truncated);
    }
}
