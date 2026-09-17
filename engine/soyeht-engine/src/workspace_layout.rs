use serde_json::{Value, json};
use soyeht_protocol::{
    Pane, PaneMode, WorkspacePaneLayoutEnvelope, WorkspacePaneLayoutKind, WorkspacePaneLayoutNode,
    WritingKind,
};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};

pub const CURRENT_VERSION: u32 = 1;

pub fn fallback_diagnostic(workspace_id: &str, layout: &Value) -> Option<Value> {
    let envelope = serde_json::from_value::<WorkspacePaneLayoutEnvelope>(layout.clone()).ok()?;
    (envelope.version != CURRENT_VERSION).then(|| {
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
    if let Some(tree) = &envelope.tree {
        validate_node(tree, &panes, &mut seen)?;
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
) -> Result<(), String> {
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
        }
        WorkspacePaneLayoutNode::V1(split) => {
            if split.children.len() < 2 {
                return Err("Workspace pane layout splits need at least two children".into());
            }
            for branch in &split.children {
                if !branch.weight.is_finite() || branch.weight <= 0.0 {
                    return Err("Workspace pane layout weights must be finite and positive".into());
                }
                validate_node(&branch.node, panes, seen)?;
            }
        }
    }
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
        Pane {
            pane_id: id.into(),
            workspace_id: "workspace".into(),
            name: id.into(),
            cwd: "/tmp".into(),
            mode: PaneMode::Terminal,
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
            writing_kind: None,
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
}
