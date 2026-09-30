use serde_json::{Value, json};
use soyeht_protocol::{
    Pane, PaneMode, WorkspacePaneLayoutBranch, WorkspacePaneLayoutCompanion,
    WorkspacePaneLayoutEnvelope, WorkspacePaneLayoutKind, WorkspacePaneLayoutNode,
    WorkspacePaneLayoutSplit, WritingKind,
};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};

pub const CURRENT_VERSION: u32 = 2;

pub fn migrate(layout: &Value) -> Option<WorkspacePaneLayoutEnvelope> {
    let mut envelope =
        serde_json::from_value::<WorkspacePaneLayoutEnvelope>(layout.clone()).ok()?;
    match envelope.version {
        1 => {
            envelope.version = CURRENT_VERSION;
            if let Some(tree) = &mut envelope.tree {
                clear_companions(tree);
            }
            Some(envelope)
        }
        CURRENT_VERSION => Some(envelope),
        _ => None,
    }
}

fn clear_companions(node: &mut WorkspacePaneLayoutNode) {
    let WorkspacePaneLayoutNode::V1(split) = node else {
        return;
    };
    split.companion = None;
    for branch in &mut split.children {
        clear_companions(&mut branch.node);
    }
}

pub fn removing_pane(layout: &Value, pane_id: &str) -> Option<Value> {
    let mut envelope = migrate(layout)?;
    let (tree, removed) = remove_from_node(envelope.tree.take(), pane_id);
    if !removed {
        return None;
    }
    envelope.tree = tree;
    serde_json::to_value(envelope).ok()
}

fn remove_from_node(
    node: Option<WorkspacePaneLayoutNode>,
    pane_id: &str,
) -> (Option<WorkspacePaneLayoutNode>, bool) {
    let Some(node) = node else {
        return (None, false);
    };
    match node {
        WorkspacePaneLayoutNode::V0(leaf) if leaf.pane_id == pane_id => (None, true),
        WorkspacePaneLayoutNode::V0(leaf) => (Some(WorkspacePaneLayoutNode::V0(leaf)), false),
        WorkspacePaneLayoutNode::V1(split) => remove_from_split(split, pane_id),
    }
}

fn remove_from_split(
    split: WorkspacePaneLayoutSplit,
    pane_id: &str,
) -> (Option<WorkspacePaneLayoutNode>, bool) {
    let companion = split.companion;
    let mut removed = false;
    let mut children = Vec::with_capacity(split.children.len());
    for branch in split.children {
        let (node, branch_removed) = remove_from_node(Some(branch.node), pane_id);
        removed |= branch_removed;
        if let Some(node) = node {
            children.push(WorkspacePaneLayoutBranch {
                weight: branch.weight,
                node,
            });
        }
    }
    if !removed {
        return (
            Some(WorkspacePaneLayoutNode::V1(WorkspacePaneLayoutSplit {
                axis: split.axis,
                children,
                companion,
            })),
            false,
        );
    }
    let mut children = children.into_iter();
    let Some(first) = children.next() else {
        return (None, true);
    };
    let Some(second) = children.next() else {
        return (Some(first.node), true);
    };
    let mut remaining = vec![first, second];
    remaining.extend(children);
    let companion = companion.filter(|pair| {
        remaining
            .iter()
            .any(|branch| node_contains_pane(&branch.node, &pair.writing_pane_id))
            && remaining
                .iter()
                .any(|branch| node_contains_pane(&branch.node, &pair.agent_pane_id))
    });
    (
        Some(WorkspacePaneLayoutNode::V1(WorkspacePaneLayoutSplit {
            axis: split.axis,
            children: remaining,
            companion,
        })),
        true,
    )
}

fn node_contains_pane(node: &WorkspacePaneLayoutNode, pane_id: &str) -> bool {
    match node {
        WorkspacePaneLayoutNode::V0(leaf) => leaf.pane_id == pane_id,
        WorkspacePaneLayoutNode::V1(split) => split
            .children
            .iter()
            .any(|branch| node_contains_pane(&branch.node, pane_id)),
    }
}

pub fn fallback_diagnostic(workspace_id: &str, layout: &Value) -> Option<Value> {
    let envelope = serde_json::from_value::<WorkspacePaneLayoutEnvelope>(layout.clone()).ok()?;
    (!matches!(envelope.version, 1 | CURRENT_VERSION)).then(|| {
        json!({
            "event":"engine.workspace_layout_version_unsupported",
            "workspaceId":workspace_id,
            "version":envelope.version,
            "fallback":"initial_by_type"
        })
    })
}

pub fn log_fallback_diagnostics<'a>(
    layouts: impl IntoIterator<Item = (&'a str, &'a Value)>,
    mut output: impl Write,
) -> io::Result<usize> {
    let mut count = 0;
    for (workspace_id, layout) in layouts {
        if let Some(diagnostic) = fallback_diagnostic(workspace_id, layout) {
            writeln!(output, "{diagnostic}")?;
            count += 1;
        }
    }
    Ok(count)
}

pub fn validate(
    workspace_id: &str,
    envelope: &WorkspacePaneLayoutEnvelope,
    panes: impl IntoIterator<Item = Pane>,
) -> Result<(), String> {
    if envelope.version != CURRENT_VERSION {
        return Err(format!(
            "Workspace pane layout version {} is unsupported",
            envelope.version
        ));
    }
    let panes: BTreeMap<_, _> = panes
        .into_iter()
        .filter(|pane| pane.workspace_id == workspace_id)
        .map(|pane| (pane.pane_id.clone(), pane))
        .collect();
    let mut seen = BTreeSet::new();
    let mut paired = BTreeSet::new();
    if let Some(tree) = &envelope.tree {
        validate_node(tree, &panes, &mut seen, &mut paired)?;
    }
    let expected: BTreeSet<_> = panes.keys().cloned().collect();
    if seen != expected {
        return Err("Workspace pane layout must contain every open workspace pane".into());
    }
    Ok(())
}

fn validate_node(
    node: &WorkspacePaneLayoutNode,
    panes: &BTreeMap<String, Pane>,
    seen: &mut BTreeSet<String>,
    paired: &mut BTreeSet<String>,
) -> Result<BTreeSet<String>, String> {
    match node {
        WorkspacePaneLayoutNode::V0(leaf) => {
            let pane = panes
                .get(&leaf.pane_id)
                .ok_or_else(|| format!("Pane {} is not open in this workspace", leaf.pane_id))?;
            if !seen.insert(leaf.pane_id.clone()) {
                return Err(format!("Pane {} appears more than once", leaf.pane_id));
            }
            if leaf.kind != kind_for(pane) {
                return Err(format!("Pane {} has the wrong layout kind", leaf.pane_id));
            }
            Ok(BTreeSet::from([leaf.pane_id.clone()]))
        }
        WorkspacePaneLayoutNode::V1(split) => {
            if split.children.len() < 2 {
                return Err("Workspace pane layout splits need at least two children".into());
            }
            let mut descendants = BTreeSet::new();
            for branch in &split.children {
                if !branch.weight.is_finite() || branch.weight <= 0.0 {
                    return Err("Workspace pane layout weights must be finite and positive".into());
                }
                descendants.extend(validate_node(&branch.node, panes, seen, paired)?);
            }
            if let Some(companion) = &split.companion {
                validate_companion(companion, split, &descendants, panes, paired)?;
            }
            Ok(descendants)
        }
    }
}

fn validate_companion(
    companion: &WorkspacePaneLayoutCompanion,
    split: &WorkspacePaneLayoutSplit,
    descendants: &BTreeSet<String>,
    panes: &BTreeMap<String, Pane>,
    paired: &mut BTreeSet<String>,
) -> Result<(), String> {
    if companion.writing_pane_id == companion.agent_pane_id {
        return Err("Writing and agent companion panes must be different".into());
    }
    let writing = panes
        .get(&companion.writing_pane_id)
        .ok_or_else(|| "Writing companion pane is not open in this workspace".to_string())?;
    let agent = panes
        .get(&companion.agent_pane_id)
        .ok_or_else(|| "Agent companion pane is not open in this workspace".to_string())?;
    if !matches!(
        kind_for(writing),
        WorkspacePaneLayoutKind::Text
            | WorkspacePaneLayoutKind::Presentation
            | WorkspacePaneLayoutKind::Screenplay
    ) {
        return Err("Writing companion pane must be a writing pane with a known kind".into());
    }
    if kind_for(agent) != WorkspacePaneLayoutKind::Agent {
        return Err("Agent companion pane must be an agent pane".into());
    }
    let members = BTreeSet::from([
        companion.writing_pane_id.clone(),
        companion.agent_pane_id.clone(),
    ]);
    if !members.is_subset(descendants) {
        return Err("A writing companion must stay inside its annotated subtree".into());
    }
    if members.iter().any(|pane_id| paired.contains(pane_id)) {
        return Err("A pane cannot belong to more than one writing companion".into());
    }
    let direct_members = split
        .children
        .iter()
        .map(|branch| match &branch.node {
            WorkspacePaneLayoutNode::V0(leaf) => Some(leaf.pane_id.clone()),
            WorkspacePaneLayoutNode::V1(_) => None,
        })
        .collect::<Option<BTreeSet<_>>>();
    if split.children.len() != 2 || direct_members.as_ref() != Some(&members) {
        return Err(
            "A writing companion must annotate exactly its two direct leaf children".into(),
        );
    }
    paired.extend(members);
    Ok(())
}

fn kind_for(pane: &Pane) -> WorkspacePaneLayoutKind {
    match pane.mode {
        PaneMode::Terminal => WorkspacePaneLayoutKind::Terminal,
        PaneMode::Agent => WorkspacePaneLayoutKind::Agent,
        PaneMode::Editor => WorkspacePaneLayoutKind::Editor,
        PaneMode::Web => WorkspacePaneLayoutKind::Web,
        PaneMode::Writing => match pane.writing_kind {
            Some(WritingKind::Text) => WorkspacePaneLayoutKind::Text,
            Some(WritingKind::Presentation) => WorkspacePaneLayoutKind::Presentation,
            Some(WritingKind::Screenplay) => WorkspacePaneLayoutKind::Screenplay,
            None => WorkspacePaneLayoutKind::UnknownWriting,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soyeht_protocol::{
        PaneState, StateSource, WorkspacePaneLayoutLeaf, WorkspacePaneLayoutMode,
    };

    fn pane(id: &str) -> Pane {
        pane_as(id, PaneMode::Terminal, None)
    }

    fn pane_as(id: &str, mode: PaneMode, writing_kind: Option<WritingKind>) -> Pane {
        Pane {
            pane_id: id.into(),
            workspace_id: "workspace".into(),
            name: id.into(),
            cwd: "/tmp".into(),
            mode,
            state: PaneState::Idle,
            source: StateSource::Protocol,
            confidence: 1.0,
            phrase: String::new(),
            generation: 1,
            cols: 80,
            rows: 24,
            record: false,
            experiment_capture: None,
            pid: None,
            stream_id: None,
            agent: None,
            url: None,
            title: None,
            viewport: None,
            incognito: None,
            web_session: None,
            writing_kind,
            writing_section: None,
        }
    }

    #[test]
    fn absent_legacy_envelope_has_no_migration_warning() {
        assert_eq!(fallback_diagnostic("workspace", &json!({"panes":[]})), None);
    }

    #[test]
    fn unknown_version_falls_back_with_a_structured_warning() {
        assert_eq!(
            fallback_diagnostic(
                "workspace",
                &json!({"version":99,"mode":"custom","tree":null})
            ),
            Some(json!({
                "event":"engine.workspace_layout_version_unsupported",
                "workspaceId":"workspace",
                "version":99,
                "fallback":"initial_by_type"
            }))
        );
    }

    #[test]
    fn migration_logs_unknown_versions_but_not_absent_envelopes() {
        let unknown = json!({"version":99,"mode":"custom","tree":null});
        let absent = json!({"panes":[]});
        let mut log = Vec::new();
        assert_eq!(
            log_fallback_diagnostics([("unknown", &unknown), ("absent", &absent)], &mut log)
                .unwrap(),
            1
        );
        let log = String::from_utf8(log).unwrap();
        assert!(log.contains("engine.workspace_layout_version_unsupported"));
        assert!(log.contains("\"workspaceId\":\"unknown\""));
        assert!(log.contains("\"fallback\":\"initial_by_type\""));
        assert!(!log.contains("\"workspaceId\":\"absent\""));
    }

    #[test]
    fn current_layout_rejects_nonexistent_panes() {
        let envelope = WorkspacePaneLayoutEnvelope {
            version: CURRENT_VERSION,
            mode: WorkspacePaneLayoutMode::Custom,
            tree: Some(WorkspacePaneLayoutNode::V0(WorkspacePaneLayoutLeaf {
                pane_id: "missing".into(),
                kind: WorkspacePaneLayoutKind::Terminal,
            })),
        };
        assert_eq!(
            validate("workspace", &envelope, [pane("real")]).unwrap_err(),
            "Pane missing is not open in this workspace"
        );
    }

    #[test]
    fn version_one_migrates_the_same_tree_with_no_companions() {
        let old = json!({
            "version":1,
            "mode":"custom",
            "tree":{
                "axis":"horizontal",
                "children":[
                    {"weight":2.0,"node":{"paneId":"writing","kind":"text"}},
                    {"weight":1.0,"node":{"paneId":"agent","kind":"agent"}}
                ],
                "companion":{"writingPaneId":"writing","agentPaneId":"agent"}
            }
        });
        let migrated = serde_json::to_value(migrate(&old).unwrap()).unwrap();
        assert_eq!(migrated["version"], 2);
        assert_eq!(migrated["tree"]["axis"], old["tree"]["axis"]);
        assert_eq!(migrated["tree"]["children"], old["tree"]["children"]);
        assert!(migrated["tree"].get("companion").is_none());
    }

    #[test]
    fn companion_validation_rejects_missing_wrong_same_duplicate_and_outside_panes() {
        let writing = pane_as("writing", PaneMode::Writing, Some(WritingKind::Text));
        let agent = pane_as("agent", PaneMode::Agent, None);
        let agent_two = pane_as("agent-2", PaneMode::Agent, None);
        let terminal = pane("terminal");

        let missing = serde_json::from_value(json!({
            "version":2,"mode":"custom",
            "tree":{"axis":"horizontal","children":[
                {"weight":1,"node":{"paneId":"writing","kind":"text"}},
                {"weight":1,"node":{"paneId":"agent","kind":"agent"}}
            ],"companion":{"writingPaneId":"missing","agentPaneId":"agent"}}
        }))
        .unwrap();
        assert_eq!(
            validate("workspace", &missing, [writing.clone(), agent.clone()]).unwrap_err(),
            "Writing companion pane is not open in this workspace"
        );

        let wrong_kind = serde_json::from_value(json!({
            "version":2,"mode":"custom",
            "tree":{"axis":"horizontal","children":[
                {"weight":1,"node":{"paneId":"terminal","kind":"terminal"}},
                {"weight":1,"node":{"paneId":"agent","kind":"agent"}}
            ],"companion":{"writingPaneId":"terminal","agentPaneId":"agent"}}
        }))
        .unwrap();
        assert_eq!(
            validate("workspace", &wrong_kind, [terminal.clone(), agent.clone()]).unwrap_err(),
            "Writing companion pane must be a writing pane with a known kind"
        );

        let same = serde_json::from_value(json!({
            "version":2,"mode":"custom",
            "tree":{"axis":"horizontal","children":[
                {"weight":1,"node":{"paneId":"writing","kind":"text"}},
                {"weight":1,"node":{"paneId":"agent","kind":"agent"}}
            ],"companion":{"writingPaneId":"writing","agentPaneId":"writing"}}
        }))
        .unwrap();
        assert_eq!(
            validate("workspace", &same, [writing.clone(), agent.clone()]).unwrap_err(),
            "Writing and agent companion panes must be different"
        );

        let duplicate = serde_json::from_value(json!({
            "version":2,"mode":"custom",
            "tree":{"axis":"horizontal","children":[
                {"weight":1,"node":{"axis":"vertical","children":[
                    {"weight":1,"node":{"paneId":"writing","kind":"text"}},
                    {"weight":1,"node":{"paneId":"agent","kind":"agent"}}
                ],"companion":{"writingPaneId":"writing","agentPaneId":"agent"}}},
                {"weight":1,"node":{"paneId":"agent-2","kind":"agent"}}
            ],"companion":{"writingPaneId":"writing","agentPaneId":"agent-2"}}
        }))
        .unwrap();
        assert_eq!(
            validate(
                "workspace",
                &duplicate,
                [writing.clone(), agent.clone(), agent_two.clone()]
            )
            .unwrap_err(),
            "A pane cannot belong to more than one writing companion"
        );

        let outside = serde_json::from_value(json!({
            "version":2,"mode":"custom",
            "tree":{"axis":"horizontal","children":[
                {"weight":1,"node":{"axis":"vertical","children":[
                    {"weight":1,"node":{"paneId":"writing","kind":"text"}},
                    {"weight":1,"node":{"paneId":"terminal","kind":"terminal"}}
                ],"companion":{"writingPaneId":"writing","agentPaneId":"agent"}}},
                {"weight":1,"node":{"paneId":"agent","kind":"agent"}}
            ]}
        }))
        .unwrap();
        assert_eq!(
            validate("workspace", &outside, [writing, terminal, agent]).unwrap_err(),
            "A writing companion must stay inside its annotated subtree"
        );
    }

    #[test]
    fn companion_validation_requires_exactly_two_direct_leaf_children() {
        let writing = pane_as("writing", PaneMode::Writing, Some(WritingKind::Text));
        let agent = pane_as("agent", PaneMode::Agent, None);
        let terminal = pane("terminal");
        let extra_child: WorkspacePaneLayoutEnvelope = serde_json::from_value(json!({
            "version":2,"mode":"custom",
            "tree":{"axis":"horizontal","children":[
                {"weight":1,"node":{"paneId":"writing","kind":"text"}},
                {"weight":1,"node":{"paneId":"agent","kind":"agent"}},
                {"weight":1,"node":{"paneId":"terminal","kind":"terminal"}}
            ],"companion":{"writingPaneId":"writing","agentPaneId":"agent"}}
        }))
        .unwrap();
        assert_eq!(
            validate(
                "workspace",
                &extra_child,
                [writing.clone(), agent.clone(), terminal.clone()]
            )
            .unwrap_err(),
            "A writing companion must annotate exactly its two direct leaf children"
        );

        let nested_member: WorkspacePaneLayoutEnvelope = serde_json::from_value(json!({
            "version":2,"mode":"custom",
            "tree":{"axis":"horizontal","children":[
                {"weight":1,"node":{"paneId":"writing","kind":"text"}},
                {"weight":1,"node":{"axis":"vertical","children":[
                    {"weight":1,"node":{"paneId":"agent","kind":"agent"}},
                    {"weight":1,"node":{"paneId":"terminal","kind":"terminal"}}
                ]}}
            ],"companion":{"writingPaneId":"writing","agentPaneId":"agent"}}
        }))
        .unwrap();
        assert_eq!(
            validate("workspace", &nested_member, [writing, agent, terminal]).unwrap_err(),
            "A writing companion must annotate exactly its two direct leaf children"
        );
    }

    #[test]
    fn closing_a_pane_prunes_its_leaf_and_collapses_single_child_splits() {
        let layout = json!({
            "version":1,
            "mode":"custom",
            "tree":{
                "axis":"horizontal",
                "children":[
                    {"weight":2,"node":{"paneId":"closed","kind":"terminal"}},
                    {"weight":1,"node":{"paneId":"open","kind":"terminal"}}
                ]
            }
        });

        assert_eq!(
            removing_pane(&layout, "closed"),
            Some(json!({
                "version":2,
                "mode":"custom",
                "tree":{"paneId":"open","kind":"terminal"}
            }))
        );
        assert_eq!(removing_pane(&layout, "absent"), None);
    }

    #[test]
    fn closing_a_pane_preserves_only_companions_whose_members_remain() {
        let layout = json!({
            "version":2,
            "mode":"custom",
            "tree":{
                "axis":"horizontal",
                "children":[
                    {"weight":2,"node":{"paneId":"writing","kind":"text"}},
                    {"weight":1,"node":{"paneId":"agent","kind":"agent"}},
                    {"weight":1,"node":{"paneId":"other","kind":"terminal"}}
                ],
                "companion":{"writingPaneId":"writing","agentPaneId":"agent"}
            }
        });

        let without_other = removing_pane(&layout, "other").unwrap();
        assert_eq!(
            without_other["tree"]["companion"],
            json!({"writingPaneId":"writing","agentPaneId":"agent"})
        );

        let without_agent = removing_pane(&layout, "agent").unwrap();
        assert!(without_agent["tree"].get("companion").is_none());
    }
}
