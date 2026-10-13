//! The APNs payload for a question an agent asks the person.
//!
//! The payload carries only what the notification shows and the identifiers
//! the phone needs to answer through `pane.permission.answer` or open the
//! pane. The phone registers the categories named here (contract 06 §1.6).

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use soyeht_protocol::event;

/// Allow and Deny actions; the phone answers without opening the app.
pub const PERMISSION_CATEGORY: &str = "soyeht.permission";
/// No actions: a tap opens the pane. Used for forms and for permissions that
/// offer no one-time allow and reject pair.
pub const ATTENTION_CATEGORY: &str = "soyeht.attention";

/// APNs refuses alert payloads larger than 4 KB.
const PAYLOAD_BYTES: usize = 4 * 1024;
/// APNs refuses an `apns-collapse-id` longer than 64 bytes.
const COLLAPSE_ID_BYTES: usize = 64;
const TITLE_CHARS: usize = 60;
const BODY_CHARS: usize = 160;

/// A durable question event and what the engine knows about its pane.
pub struct Question<'a> {
    pub event: &'a str,
    pub request: &'a Value,
    pub workspace_id: &'a str,
    pub harness: &'a str,
    pub engine_id: &'a str,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Alert {
    pub request_id: String,
    pub collapse_id: String,
    /// The request's own deadline; APNs stops trying after it.
    pub expires_unix: i64,
    pub payload: Value,
}

/// Collapses whitespace and control characters and cuts at a character
/// boundary, so a notification never shows raw newlines or a split glyph.
fn short(text: &str, limit: usize) -> String {
    let clean = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if clean.chars().count() <= limit {
        return clean;
    }
    let mut cut: String = clean.chars().take(limit - 1).collect();
    cut.truncate(cut.trim_end().len());
    cut.push('…');
    cut
}

fn option_id<'a>(request: &'a Value, kind: &str) -> Option<&'a str> {
    request["options"]
        .as_array()?
        .iter()
        .find(|option| option["kind"] == kind)?["optionId"]
        .as_str()
}

/// The tool and its target: the command that would run, else the path.
fn permission_body(request: &Value) -> String {
    let title = request["title"].as_str().unwrap_or_default().trim();
    let presentation = &request["presentation"];
    let target = presentation["command"]
        .as_str()
        .or_else(|| presentation["targetPath"].as_str())
        .or_else(|| request["domain"].as_str())
        .map(str::trim)
        .filter(|target| !target.is_empty() && *target != title);
    match target {
        Some(target) if !title.is_empty() => format!("{title} · {target}"),
        Some(target) => target.to_owned(),
        None => title.to_owned(),
    }
}

fn collapse_id(request_id: &str) -> String {
    if request_id.len() <= COLLAPSE_ID_BYTES {
        request_id.to_owned()
    } else {
        format!("{:x}", Sha256::digest(request_id.as_bytes()))
    }
}

/// Builds the alert, or `None` when the event is not a question or the
/// request lacks the identifiers the phone needs.
pub fn alert(question: &Question<'_>) -> Option<Alert> {
    let request = question.request;
    let request_id = request["requestId"].as_str()?;
    let pane_id = request["paneId"].as_str()?;
    let expires_unix = request["expiresAt"]
        .as_str()
        .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
        .map_or(0, |at| at.timestamp());
    let harness = short(question.harness, TITLE_CHARS);
    let mut custom = Map::new();
    custom.insert("engineId".into(), json!(question.engine_id));
    custom.insert("workspaceId".into(), json!(question.workspace_id));
    custom.insert("paneId".into(), json!(pane_id));
    custom.insert("requestId".into(), json!(request_id));
    let (title, body, category) = match question.event {
        event::PANE_PERMISSION_REQUESTED => {
            custom.insert("kind".into(), json!("permission"));
            let answers = option_id(request, "allow_once").zip(option_id(request, "reject_once"));
            let category = match answers {
                Some((allow, deny)) => {
                    custom.insert("allowOptionId".into(), json!(allow));
                    custom.insert("denyOptionId".into(), json!(deny));
                    PERMISSION_CATEGORY
                }
                None => ATTENTION_CATEGORY,
            };
            (
                format!("{harness} needs approval"),
                permission_body(request),
                category,
            )
        }
        event::PANE_ELICITATION_REQUESTED => {
            custom.insert("kind".into(), json!("question"));
            (
                format!("{harness} has a question"),
                request["title"].as_str().unwrap_or_default().to_owned(),
                ATTENTION_CATEGORY,
            )
        }
        _ => return None,
    };
    let payload = json!({
        "aps": {
            "alert": {"title": short(&title, TITLE_CHARS), "body": short(&body, BODY_CHARS)},
            "category": category,
            "sound": "default",
            "thread-id": pane_id,
        },
        "soyeht": custom,
    });
    if serde_json::to_vec(&payload).ok()?.len() > PAYLOAD_BYTES {
        return None;
    }
    Some(Alert {
        request_id: request_id.to_owned(),
        collapse_id: collapse_id(request_id),
        expires_unix,
        payload,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const VECTOR: &str = include_str!("../../../../contracts/vectors/push-permission.json");

    fn permission(command: &str) -> Value {
        json!({
            "requestId":"6f1c7a2e-0d5b-4e8f-9a51-2c3d4e5f6a7b",
            "paneId":"0b9f1e4c-5a6d-4c3b-8e2f-1a2b3c4d5e6f",
            "generation":1,"promptId":"prompt","title":"Run command","detail":"",
            "presentation":{"command":command,"fileChanges":[],"previewOmitted":false},
            "options":[
                {"optionId":"allow","kind":"allow_once","name":"Allow"},
                {"optionId":"always","kind":"allow_always","name":"Always allow"},
                {"optionId":"reject","kind":"reject_once","name":"Reject"}
            ],
            "expiresAt":"2026-10-01T12:30:00.000Z"
        })
    }

    fn question<'a>(event: &'a str, request: &'a Value) -> Question<'a> {
        Question {
            event,
            request,
            workspace_id: "3c2b1a0f-9e8d-4c7b-a6f5-e4d3c2b1a0f9",
            harness: "Claude",
            engine_id: "9a8b7c6d-5e4f-4a3b-8c2d-1e0f9a8b7c6d",
        }
    }

    #[test]
    fn permission_payload_matches_the_shared_vector() {
        let request = permission("git push origin main");
        let alert = alert(&question("pane.permission_requested", &request)).unwrap();
        let vector: Value = serde_json::from_str(VECTOR).unwrap();
        assert_eq!(alert.payload, vector);
        assert_eq!(alert.collapse_id, "6f1c7a2e-0d5b-4e8f-9a51-2c3d4e5f6a7b");
        assert_eq!(alert.expires_unix, 1_790_857_800);
    }

    #[test]
    fn body_is_one_line_and_truncated_at_a_character_boundary() {
        let command = format!("echo {}\nrm -rf build", "é".repeat(300));
        let request = permission(&command);
        let alert = alert(&question("pane.permission_requested", &request)).unwrap();
        let body = alert.payload["aps"]["alert"]["body"].as_str().unwrap();
        assert_eq!(body.chars().count(), BODY_CHARS);
        assert!(body.starts_with("Run command · echo éé"));
        assert!(body.ends_with('…'));
        assert!(!body.contains('\n'));
    }

    #[test]
    fn a_path_is_the_target_when_nothing_runs() {
        let mut request = permission("");
        request["title"] = json!("Edit file");
        request["presentation"] =
            json!({"targetPath":"/repo/src/main.rs","fileChanges":[],"previewOmitted":false});
        let alert = alert(&question("pane.permission_requested", &request)).unwrap();
        assert_eq!(
            alert.payload["aps"]["alert"]["body"],
            "Edit file · /repo/src/main.rs"
        );
    }

    #[test]
    fn a_permission_without_one_time_answers_only_opens_the_pane() {
        let mut request = permission("ls");
        request["options"] = json!([{"optionId":"always","kind":"allow_always","name":"Always"}]);
        let alert = alert(&question("pane.permission_requested", &request)).unwrap();
        assert_eq!(alert.payload["aps"]["category"], ATTENTION_CATEGORY);
        assert!(alert.payload["soyeht"].get("allowOptionId").is_none());
    }

    #[test]
    fn a_form_question_opens_the_pane() {
        let request = json!({
            "requestId":"r","paneId":"p","generation":1,"promptId":"x",
            "title":"Which branch\nshould I use?","schema":{},"expiresAt":"bad"
        });
        let alert = alert(&question("pane.elicitation_requested", &request)).unwrap();
        assert_eq!(
            alert.payload["aps"]["alert"]["title"],
            "Claude has a question"
        );
        assert_eq!(
            alert.payload["aps"]["alert"]["body"],
            "Which branch should I use?"
        );
        assert_eq!(alert.payload["aps"]["category"], ATTENTION_CATEGORY);
        assert_eq!(alert.payload["soyeht"]["kind"], "question");
        assert_eq!(alert.expires_unix, 0);
    }

    #[test]
    fn long_request_ids_collapse_to_a_bounded_digest() {
        let mut request = permission("ls");
        request["requestId"] = json!("r".repeat(128));
        let alert = alert(&question("pane.permission_requested", &request)).unwrap();
        assert_eq!(alert.collapse_id.len(), COLLAPSE_ID_BYTES);
        assert_eq!(alert.payload["soyeht"]["requestId"], "r".repeat(128));
    }

    #[test]
    fn other_events_and_incomplete_requests_produce_nothing() {
        let request = permission("ls");
        assert!(alert(&question("pane.updated", &request)).is_none());
        let missing = json!({"paneId":"p"});
        assert!(alert(&question("pane.permission_requested", &missing)).is_none());
    }
}
