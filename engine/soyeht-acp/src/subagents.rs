//! Harness subagents over ACP.
//!
//! Both pinned adapters implement the ACP subagent draft
//! (agentclientprotocol/agent-client-protocol#1992): the parent session reports
//! `subagent_spawned` and `subagent_state_update`, and each subagent's own
//! activity arrives as ordinary updates under its `subagentSessionId`. The
//! published TypeScript SDK strips the draft `subagents` client capability, so
//! the adapters also accept the JetBrains AIR `nativeSubagentSessions`
//! capability, which travels in `_meta`. `client::initialize_params` advertises both.
use crate::Error;
use serde_json::Value;

const NAME_CHARS: usize = 256;
const TASK_CHARS: usize = 4096;
const MODEL_CHARS: usize = 128;

#[derive(Debug, Clone, PartialEq)]
pub struct Spawned {
    pub session_id: String,
    pub name: String,
    pub task: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Outcome {
    Completed,
    Failed,
    Cancelled,
    Disconnected,
}

/// A subagent's lifecycle as its parent session reports it.
#[derive(Debug, Clone, PartialEq)]
pub enum Lifecycle {
    Spawned(Spawned),
    Finished(String, Outcome),
}

/// What a harness reports about a finished subagent on the spawning tool call.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub session_id: String,
    pub model: Option<String>,
}

fn bounded(value: &str, limit: usize) -> String {
    let value = value.trim();
    match value.char_indices().nth(limit) {
        Some((end, _)) => format!("{}…", &value[..end]),
        None => value.into(),
    }
}

fn valid_session_id(id: &str) -> bool {
    !id.is_empty() && id.chars().count() <= 128 && !id.contains('\0')
}

fn session_id(update: &Value) -> Result<String, Error> {
    update["subagentSessionId"]
        .as_str()
        .filter(|id| valid_session_id(id))
        .map(str::to_owned)
        .ok_or(Error::MissingSubagentId)
}

/// Parses `subagent_spawned`.
pub fn spawned(update: &Value) -> Result<Lifecycle, Error> {
    let name = update["name"].as_str().unwrap_or_default().trim();
    Ok(Lifecycle::Spawned(Spawned {
        session_id: session_id(update)?,
        name: bounded(if name.is_empty() { "Subagent" } else { name }, NAME_CHARS),
        task: bounded(update["task"].as_str().unwrap_or_default(), TASK_CHARS),
    }))
}

/// Parses `subagent_state_update`.
pub fn finished(update: &Value) -> Result<Lifecycle, Error> {
    let outcome = match update["state"].as_str() {
        Some("completed") => Outcome::Completed,
        Some("failed") => Outcome::Failed,
        Some("cancelled") => Outcome::Cancelled,
        Some("disconnected") => Outcome::Disconnected,
        _ => return Err(Error::InvalidSubagentState),
    };
    Ok(Lifecycle::Finished(session_id(update)?, outcome))
}

/// Claude Code's Agent tool reports the subagent it ran and the model it
/// resolved in `_meta.claudeCode.toolResponse`. The subagent's session is the
/// reported agent ID.
pub fn report(tool: &Value) -> Option<Report> {
    let response = &tool["_meta"]["claudeCode"]["toolResponse"];
    let session_id = response["agentId"]
        .as_str()
        .filter(|id| valid_session_id(id))?;
    Some(Report {
        session_id: session_id.into(),
        model: response["resolvedModel"]
            .as_str()
            .map(str::trim)
            .filter(|model| !model.is_empty())
            .map(|model| bounded(model, MODEL_CHARS)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn adapter_shapes_parse_into_bounded_subagent_facts() {
        let long = "x".repeat(TASK_CHARS + 10);
        let Lifecycle::Spawned(spawned) = spawned(&json!({"sessionUpdate":"subagent_spawned",
            "subagentSessionId":"child","name":"  ","task":long,"capabilities":{}}))
        .unwrap() else {
            panic!("spawn parsed as another lifecycle report");
        };
        assert_eq!(spawned.name, "Subagent");
        assert_eq!(spawned.task.chars().count(), TASK_CHARS + 1);
        assert!(super::spawned(&json!({"subagentSessionId":""})).is_err());
        assert_eq!(
            finished(&json!({"subagentSessionId":"child","state":"failed"})).unwrap(),
            Lifecycle::Finished("child".into(), Outcome::Failed)
        );
        assert!(finished(&json!({"subagentSessionId":"child","state":"running"})).is_err());
        let tool = json!({"toolCallId":"toolu_1","_meta":{"claudeCode":{"toolName":"Agent",
            "toolResponse":{"status":"completed","agentId":"child","resolvedModel":"claude-model"}}}});
        assert_eq!(
            report(&tool),
            Some(Report {
                session_id: "child".into(),
                model: Some("claude-model".into())
            })
        );
        assert_eq!(report(&json!({"toolCallId":"toolu_2"})), None);
    }
}
