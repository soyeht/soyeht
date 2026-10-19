use soyeht_protocol::{WebRect, WebSnapshotNode, limits};

use crate::{Error, NavigationReferences, Result};

const RESPONSE_HEADROOM: usize = 4096;

#[derive(Debug, Clone, PartialEq)]
pub struct SnapshotNodeInput {
    pub role: String,
    pub name: String,
    pub state: String,
    pub value: Option<String>,
    pub rect: WebRect,
    pub parent_index: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BoundedSnapshot {
    pub nodes: Vec<WebSnapshotNode>,
    pub truncated: bool,
}

pub fn build_snapshot(
    references: &mut NavigationReferences,
    inputs: impl IntoIterator<Item = SnapshotNodeInput>,
) -> Result<BoundedSnapshot> {
    let budget = limits::WEB_SNAPSHOT_BYTES.saturating_sub(RESPONSE_HEADROOM);
    let mut nodes = Vec::new();
    let mut encoded_bytes = 2_usize;
    let mut truncated = false;
    for input in inputs {
        validate_input(&input, nodes.len())?;
        let reference = references.next_reference();
        let parent_ref = input
            .parent_index
            .and_then(|index| nodes.get(index))
            .map(|node: &WebSnapshotNode| node.r#ref.clone());
        let node = WebSnapshotNode {
            r#ref: reference,
            role: input.role,
            name: input.name,
            state: input.state,
            value: input.value,
            rect: input.rect,
            parent_ref,
        };
        let node_bytes = serde_json::to_vec(&node).map_err(|_| Error::InvalidSnapshotNode)?;
        let separator = usize::from(!nodes.is_empty());
        if encoded_bytes
            .saturating_add(separator)
            .saturating_add(node_bytes.len())
            > budget
        {
            truncated = true;
            break;
        }
        let issued = references.issue()?;
        if issued != node.r#ref {
            return Err(Error::InvalidSnapshotNode);
        }
        encoded_bytes = encoded_bytes
            .saturating_add(separator)
            .saturating_add(node_bytes.len());
        nodes.push(node);
    }
    Ok(BoundedSnapshot { nodes, truncated })
}

fn validate_input(input: &SnapshotNodeInput, accepted: usize) -> Result<()> {
    let valid = input.role.len() <= limits::WEB_TOKEN_BYTES
        && input.name.len() <= limits::WEB_LABEL_BYTES
        && input.state.len() <= limits::WEB_LABEL_BYTES
        && input
            .value
            .as_ref()
            .is_none_or(|value| value.len() <= limits::WEB_FIELD_BYTES)
        && input.parent_index.is_none_or(|index| index < accepted)
        && input.rect.width as usize <= limits::WEB_EXTENT_MAX
        && input.rect.height as usize <= limits::WEB_EXTENT_MAX;
    if valid {
        Ok(())
    } else {
        Err(Error::InvalidSnapshotNode)
    }
}
