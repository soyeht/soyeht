//! The readiness checklist of the welcome screen: what the engine says this machine has, with the
//! one row only the client can answer, whether it reaches the engine at all. The rule is the
//! Mac's `ReadinessChecklist`; the cases of `contracts/vectors/readiness.json` check both.

use soyeht_protocol::{EngineReadinessResult, ReadinessItem, ReadinessKind};

/// The row for the connection to the engine.
pub const ENGINE_ID: &str = "engine";
const ENGINE_TITLE: &str = "Soyeht engine";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Engine,
    Tool,
    Harness,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowState {
    Ready,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub title: String,
    pub kind: RowKind,
    pub required: bool,
    pub state: RowState,
    /// The command that installs it on this machine, while it is missing.
    pub hint: Option<String>,
    /// What the command needs first (Homebrew), while it is missing.
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checklist {
    pub rows: Vec<Row>,
    /// Everything the app needs is here: the engine, every required tool and an agent.
    pub complete: bool,
    /// The required rows that are missing.
    pub blockers: usize,
    pub headline: String,
}

/// The checklist for a client that is (`connected`) or is not connected to the engine, and the
/// engine's latest answer, if one arrived.
pub fn checklist(connected: bool, result: Option<&EngineReadinessResult>) -> Checklist {
    let engine = Row {
        id: ENGINE_ID.into(),
        title: ENGINE_TITLE.into(),
        kind: RowKind::Engine,
        required: true,
        state: if connected {
            RowState::Ready
        } else {
            RowState::Missing
        },
        hint: None,
        note: None,
    };
    let items = result.filter(|_| connected).map(|result| &result.items);
    let mut rows = vec![engine];
    rows.extend(items.into_iter().flatten().map(row));
    let blockers = rows
        .iter()
        .filter(|row| row.required && row.state == RowState::Missing)
        .count();
    let has_agent = rows
        .iter()
        .any(|row| row.kind == RowKind::Harness && row.state == RowState::Ready);
    let headline = if !connected {
        "The Soyeht engine is not running".to_owned()
    } else if items.is_none() {
        "Checking this machine".to_owned()
    } else if blockers > 0 {
        format!(
            "{blockers} required {} missing",
            if blockers == 1 { "item" } else { "items" }
        )
    } else if !has_agent {
        "Install an agent to start".to_owned()
    } else {
        "Everything is ready".to_owned()
    };
    Checklist {
        complete: connected && items.is_some() && blockers == 0 && has_agent,
        rows,
        blockers,
        headline,
    }
}

fn row(item: &ReadinessItem) -> Row {
    Row {
        id: item.id.clone(),
        title: item.name.clone(),
        kind: match item.kind {
            ReadinessKind::Tool => RowKind::Tool,
            ReadinessKind::Harness => RowKind::Harness,
        },
        required: item.required,
        state: if item.found {
            RowState::Ready
        } else {
            RowState::Missing
        },
        hint: item.hint.clone().filter(|_| !item.found),
        note: item.note.clone().filter(|_| !item.found),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const VECTORS: &str = include_str!("../../../../../contracts/vectors/readiness.json");

    fn kind(name: &str) -> RowKind {
        match name {
            "engine" => RowKind::Engine,
            "tool" => RowKind::Tool,
            _ => RowKind::Harness,
        }
    }

    fn expected(rows: &Value) -> Vec<Row> {
        rows.as_array()
            .unwrap()
            .iter()
            .map(|row| Row {
                id: row["id"].as_str().unwrap().into(),
                title: row["title"].as_str().unwrap().into(),
                kind: kind(row["kind"].as_str().unwrap()),
                required: row["required"].as_bool().unwrap(),
                state: if row["state"] == "ready" {
                    RowState::Ready
                } else {
                    RowState::Missing
                },
                hint: row["hint"].as_str().map(str::to_owned),
                note: row["note"].as_str().map(str::to_owned),
            })
            .collect()
    }

    #[test]
    fn every_shared_case_gives_the_same_checklist() {
        let vectors: Value = serde_json::from_str(VECTORS).unwrap();
        let cases = vectors["cases"].as_array().unwrap();
        assert!(cases.len() >= 11, "the vector file lost its cases");
        for case in cases {
            let result = (!case["result"].is_null())
                .then(|| serde_json::from_value::<EngineReadinessResult>(case["result"].clone()))
                .transpose()
                .unwrap();
            let checklist = checklist(case["connected"].as_bool().unwrap(), result.as_ref());
            let want = &case["expected"];
            let name = case["name"].as_str().unwrap();
            assert_eq!(checklist.rows, expected(&want["rows"]), "{name}");
            assert_eq!(checklist.complete, want["complete"], "{name}");
            assert_eq!(checklist.blockers as u64, want["blockers"], "{name}");
            assert_eq!(
                checklist.headline,
                want["headline"].as_str().unwrap(),
                "{name}"
            );
        }
    }
}
